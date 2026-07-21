from __future__ import annotations

import re
import subprocess
from pathlib import Path

PATTERNS = {
    "private key PEM": re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    "GitHub token": re.compile(r"\bgh[oprsu]_[A-Za-z0-9]{30,}\b"),
    "AWS access key": re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    "payment live secret": re.compile(r"\bsk_live_[A-Za-z0-9]{16,}\b"),
    "Keygen administrative token": re.compile(r"\bkeygen-[A-Za-z0-9_-]{24,}\b", re.IGNORECASE),
}

ALLOWED_TEST_PRIVATE_KEY = Path(
    "CrossPlatformImageFilter/product/policy/test-keys/development-private-key.json"
)


def repository_files(repository: Path) -> list[Path]:
    output = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=repository,
        capture_output=True,
        check=True,
    ).stdout
    return [repository / Path(item.decode()) for item in output.split(b"\0") if item]


def main() -> None:
    repository = Path(__file__).resolve().parents[3]
    findings: list[str] = []
    for path in repository_files(repository):
        if not path.is_file() or path.stat().st_size > 2_000_000:
            continue
        relative = path.relative_to(repository)
        content = path.read_text(encoding="utf-8", errors="ignore")
        if relative == ALLOWED_TEST_PRIVATE_KEY:
            if '"testOnly": true' not in content:
                findings.append(f"{relative}: test key is not explicitly marked testOnly")
            continue
        for name, pattern in PATTERNS.items():
            if pattern.search(content):
                findings.append(f"{relative}: possible {name}")
    if findings:
        raise SystemExit("Repository secret scan failed:\n" + "\n".join(findings))
    print("Repository secret scan passed")


if __name__ == "__main__":
    main()
