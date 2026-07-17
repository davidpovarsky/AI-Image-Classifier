from __future__ import annotations

import json
import sqlite3
import threading
import time
from collections import OrderedDict
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from ..domain.models import FilterAction, FilterDecision

SCHEMA_VERSION = 2


@dataclass(slots=True)
class CacheEntry:
    response: dict[str, Any]
    decision: FilterDecision


class AnalysisCache:
    def __init__(
        self,
        enabled: bool,
        memory_entries: int,
        sqlite_path: Path,
        ttl_seconds: int,
        *,
        namespace: str,
        policy_fingerprint: str,
        manifest_fingerprint: str,
    ) -> None:
        self.enabled = enabled
        self.memory_entries = max(1, memory_entries)
        self.sqlite_path = sqlite_path
        self.ttl_seconds = ttl_seconds
        self.namespace = namespace
        self.policy_fingerprint = policy_fingerprint
        self.manifest_fingerprint = manifest_fingerprint
        self._memory: OrderedDict[str, CacheEntry] = OrderedDict()
        self._lock = threading.RLock()
        if enabled:
            sqlite_path.parent.mkdir(parents=True, exist_ok=True)
            with self._connect() as connection:
                connection.execute(
                    """
                    CREATE TABLE IF NOT EXISTS analysis_cache_v2 (
                        cache_key TEXT PRIMARY KEY,
                        schema_version INTEGER NOT NULL,
                        namespace TEXT NOT NULL,
                        created_at INTEGER NOT NULL,
                        last_accessed_at INTEGER NOT NULL,
                        content_hash TEXT NOT NULL,
                        policy_fingerprint TEXT NOT NULL,
                        manifest_fingerprint TEXT NOT NULL,
                        response_json TEXT NOT NULL,
                        decision_json TEXT NOT NULL
                    )
                    """
                )
                connection.execute(
                    "CREATE INDEX IF NOT EXISTS cache_v2_namespace ON analysis_cache_v2(namespace)"
                )

    def _connect(self) -> sqlite3.Connection:
        connection = sqlite3.connect(self.sqlite_path, timeout=10)
        connection.execute("PRAGMA journal_mode=WAL")
        connection.execute("PRAGMA synchronous=NORMAL")
        connection.execute("PRAGMA busy_timeout=10000")
        return connection

    def _key(self, content_hash: str) -> str:
        return f"{self.namespace}:{content_hash}"

    def get(self, content_hash: str) -> CacheEntry | None:
        if not self.enabled:
            return None
        key = self._key(content_hash)
        now = int(time.time())
        with self._lock:
            entry = self._memory.get(key)
            if entry:
                self._memory.move_to_end(key)
                return entry
            with self._connect() as connection:
                row = connection.execute(
                    """
                    SELECT created_at, response_json, decision_json
                    FROM analysis_cache_v2
                    WHERE cache_key=? AND schema_version=? AND namespace=?
                    """,
                    (key, SCHEMA_VERSION, self.namespace),
                ).fetchone()
                if row is None:
                    return None
                if now - int(row[0]) > self.ttl_seconds:
                    connection.execute("DELETE FROM analysis_cache_v2 WHERE cache_key=?", (key,))
                    return None
                try:
                    response = json.loads(row[1])
                    decision_data = json.loads(row[2])
                    if not isinstance(response, dict) or not isinstance(decision_data, dict):
                        raise ValueError("cached JSON is not an object")
                    entry = CacheEntry(
                        response=response,
                        decision=FilterDecision(
                            action=FilterAction(decision_data["action"]),
                            reasons=tuple(decision_data.get("reasons", [])),
                            evidence_ids=tuple(decision_data.get("evidenceIds", [])),
                            model_evidence_complete=bool(
                                decision_data.get("modelEvidenceComplete", True)
                            ),
                        ),
                    )
                except (json.JSONDecodeError, KeyError, TypeError, ValueError):
                    connection.execute("DELETE FROM analysis_cache_v2 WHERE cache_key=?", (key,))
                    return None
                connection.execute(
                    "UPDATE analysis_cache_v2 SET last_accessed_at=? WHERE cache_key=?",
                    (now, key),
                )
                self._remember(key, entry)
                return entry

    def put(self, content_hash: str, response: dict[str, Any], decision: FilterDecision) -> None:
        if not self.enabled:
            return
        key = self._key(content_hash)
        entry = CacheEntry(response=response, decision=decision)
        now = int(time.time())
        with self._lock:
            self._remember(key, entry)
            with self._connect() as connection:
                connection.execute(
                    """
                    INSERT INTO analysis_cache_v2(
                      cache_key, schema_version, namespace, created_at, last_accessed_at,
                      content_hash, policy_fingerprint, manifest_fingerprint,
                      response_json, decision_json
                    ) VALUES(?,?,?,?,?,?,?,?,?,?)
                    ON CONFLICT(cache_key) DO UPDATE SET
                      schema_version=excluded.schema_version,
                      namespace=excluded.namespace,
                      created_at=excluded.created_at,
                      last_accessed_at=excluded.last_accessed_at,
                      content_hash=excluded.content_hash,
                      policy_fingerprint=excluded.policy_fingerprint,
                      manifest_fingerprint=excluded.manifest_fingerprint,
                      response_json=excluded.response_json,
                      decision_json=excluded.decision_json
                    """,
                    (
                        key,
                        SCHEMA_VERSION,
                        self.namespace,
                        now,
                        now,
                        content_hash,
                        self.policy_fingerprint,
                        self.manifest_fingerprint,
                        json.dumps(response, separators=(",", ":"), ensure_ascii=False),
                        json.dumps(decision.to_dict(), separators=(",", ":"), ensure_ascii=False),
                    ),
                )

    def clear(self) -> int:
        with self._lock:
            memory_count = len(self._memory)
            self._memory.clear()
            if not self.enabled:
                return memory_count
            with self._connect() as connection:
                cursor = connection.execute(
                    "DELETE FROM analysis_cache_v2 WHERE namespace=?", (self.namespace,)
                )
                return max(memory_count, cursor.rowcount)

    def stats(self) -> dict[str, Any]:
        with self._lock:
            sqlite_entries = 0
            if self.enabled:
                with self._connect() as connection:
                    sqlite_entries = int(
                        connection.execute(
                            "SELECT COUNT(*) FROM analysis_cache_v2 WHERE namespace=?",
                            (self.namespace,),
                        ).fetchone()[0]
                    )
            return {
                "enabled": self.enabled,
                "schemaVersion": SCHEMA_VERSION,
                "namespace": self.namespace,
                "memoryEntries": len(self._memory),
                "sqliteEntries": sqlite_entries,
                "sqlitePath": str(self.sqlite_path),
                "ttlSeconds": self.ttl_seconds,
            }

    def _remember(self, key: str, entry: CacheEntry) -> None:
        self._memory[key] = entry
        self._memory.move_to_end(key)
        while len(self._memory) > self.memory_entries:
            self._memory.popitem(last=False)
