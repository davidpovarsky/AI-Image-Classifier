from __future__ import annotations

import http.client
import io
import socket
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest
from PIL import Image


def free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def png() -> bytes:
    stream = io.BytesIO()
    Image.new("RGB", (32, 24), "white").save(stream, format="PNG")
    return stream.getvalue()


SAFE_IMAGE = png()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path == "/safe.png":
            content, mime = SAFE_IMAGE, "image/png"
        elif self.path == "/broken.png":
            content, mime = b"broken", "image/png"
        else:
            content, mime = b"plain text", "text/plain"
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(content)))
        self.send_header("ETag", "source-etag")
        self.end_headers()
        self.wfile.write(content)

    def log_message(self, format: str, *args: object) -> None:
        return


def wait_for_port(port: int, process: subprocess.Popen[str]) -> None:
    for _ in range(100):
        if process.poll() is not None:
            stdout, stderr = process.communicate(timeout=5)
            raise AssertionError(f"mitmdump exited early:\n{stdout}\n{stderr}")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                return
        except OSError:
            time.sleep(0.05)
    raise AssertionError("mitmdump did not open its proxy port")


def proxy_get(proxy_port: int, upstream_port: int, path: str) -> tuple[int, dict[str, str], bytes]:
    connection = http.client.HTTPConnection("127.0.0.1", proxy_port, timeout=30)
    connection.request("GET", f"http://127.0.0.1:{upstream_port}{path}")
    response = connection.getresponse()
    body = response.read()
    headers = {key.lower(): value for key, value in response.getheaders()}
    connection.close()
    return response.status, headers, body


def test_real_mitmproxy_addon_round_trip(tmp_path: Path) -> None:
    executable = Path(sys.executable).parent / (
        "mitmdump.exe" if sys.platform == "win32" else "mitmdump"
    )
    if not executable.is_file():
        pytest.skip("mitmdump is not installed in the test environment")
    root = Path(__file__).resolve().parents[2]
    config = tmp_path / "mock.toml"
    config.write_text(
        """
[runtime]
mock_models = true
[proxy]
ignore_hosts = []
[cache]
enabled = false
[diagnostics]
enabled = false
""".strip()
        + "\n",
        encoding="utf-8",
    )
    upstream_port = free_port()
    proxy_port = free_port()
    server = ThreadingHTTPServer(("127.0.0.1", upstream_port), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    command = [
        str(executable),
        "--quiet",
        "--listen-host",
        "127.0.0.1",
        "--listen-port",
        str(proxy_port),
        "--set",
        f"local_image_filter_config={config}",
        "-s",
        str(root / "src" / "local_image_filter" / "proxy" / "addon.py"),
    ]
    creationflags = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0
    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        creationflags=creationflags,
    )
    try:
        wait_for_port(proxy_port, process)
        status, headers, body = proxy_get(proxy_port, upstream_port, "/safe.png")
        assert status == 200
        assert body == SAFE_IMAGE
        assert headers["x-local-image-filter"] == "allow"
        status, headers, body = proxy_get(proxy_port, upstream_port, "/broken.png")
        assert status == 200
        assert headers["content-type"].startswith("image/png")
        assert headers["x-local-image-filter"] == "replace"
        assert headers["cache-control"] == "no-store"
        assert body.startswith(b"\x89PNG")
        _, headers, body = proxy_get(proxy_port, upstream_port, "/plain")
        assert body == b"plain text"
        assert "x-local-image-filter" not in headers
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        server.shutdown()
        server.server_close()
