from __future__ import annotations

import subprocess
import sys
from pathlib import Path

from product.scripts.scan_repository_secrets import repository_files


def test_repository_secret_scan() -> None:
    root = Path(__file__).resolve().parents[3]
    subprocess.run(
        [sys.executable, str(root / "product/scripts/scan_repository_secrets.py")],
        cwd=root.parent,
        check=True,
    )


def test_secret_scan_includes_untracked_nonignored_files(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q"], cwd=tmp_path, check=True)
    tracked = tmp_path / "tracked.txt"
    tracked.write_text("tracked", encoding="utf-8")
    subprocess.run(["git", "add", "tracked.txt"], cwd=tmp_path, check=True)
    (tmp_path / "new.txt").write_text("new", encoding="utf-8")
    (tmp_path / "ignored.txt").write_text("ignored", encoding="utf-8")
    (tmp_path / ".gitignore").write_text("ignored.txt\n", encoding="utf-8")

    relative = {path.relative_to(tmp_path) for path in repository_files(tmp_path)}

    assert Path("tracked.txt") in relative
    assert Path("new.txt") in relative
    assert Path("ignored.txt") not in relative
