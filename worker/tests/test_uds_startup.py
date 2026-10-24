"""Startup uses the real Unix transport with CA-file reads denied, like Seatbelt."""
import asyncio
import json
from pathlib import Path
import ssl
import tempfile

import httpx
import pytest
from plantool_worker.app import create_app, VERSIONS


@pytest.mark.asyncio
async def test_worker_starts_over_uds_without_ca_files_or_environment_proxies(monkeypatch):
    def denied(*args, **kwargs):
        raise PermissionError("sandbox denies CA-file access")

    monkeypatch.setattr(ssl, "create_default_context", denied)
    monkeypatch.setattr(ssl.SSLContext, "load_verify_locations", denied)
    for name in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
        monkeypatch.setenv(name, "http://127.0.0.1:9")
    monkeypatch.setenv("NO_PROXY", "")
    monkeypatch.setenv("SSL_CERT_FILE", "/unavailable/cacert.pem")
    monkeypatch.setenv("SSL_CERT_DIR", "/unavailable/certs")

    contexts = []
    original = httpx.AsyncHTTPTransport

    def observed_transport(*args, **kwargs):
        contexts.append(kwargs["verify"])
        return original(*args, **kwargs)

    monkeypatch.setattr(httpx, "AsyncHTTPTransport", observed_transport)
    seen = []

    async def health(reader, writer):
        try:
            request = await reader.readuntil(b"\r\n\r\n")
            seen.append(request)
            payload = json.dumps(VERSIONS).encode()
            writer.write(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n"
                         + f"Content-Length: {len(payload)}\r\nConnection: close\r\n\r\n".encode()
                         + payload)
            await writer.drain()
        finally:
            writer.close()
            await writer.wait_closed()

    # Keep the socket below macOS's Unix path-length limit.
    with tempfile.TemporaryDirectory(prefix="pt-uds-", dir="/tmp") as directory:
        home = Path(directory)
        (home / "run").mkdir()
        async with await asyncio.start_unix_server(health, path=home/"run/daemon.sock"):
            app = create_app(home, "fake-worker-credential")
            async with app.router.lifespan_context(app):
                assert len(seen) == 1
                assert b"GET /internal/v1/health HTTP/1.1" in seen[0]
                assert b"x-plantool-run-token: fake-worker-credential" in seen[0]
                assert f"x-plantool-contract-version: {VERSIONS['contract_version']}".encode() in seen[0]
        assert len(contexts) == 1
        assert contexts[0].verify_mode == ssl.CERT_REQUIRED
        assert contexts[0].check_hostname is True
