#!/usr/bin/env python3
"""Exercise capture uploads and the CLI against a running site's real storage."""
from __future__ import annotations

import argparse
import gzip
import http.client
import json
import os
from pathlib import Path
import subprocess
import urllib.error
import urllib.parse
import urllib.request


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--fixture", type=Path, default=Path(__file__).resolve().parents[1] / "crates/horizon-perf-format/tests/fixtures/v1.hzperf")
    args = parser.parse_args()
    base = args.url.rstrip("/")
    raw = args.fixture.read_bytes()
    owners: list[tuple[str, str]] = []

    def request(method: str, path: str, data: bytes | None = None, token: str | None = None):
        headers = {"Content-Type": "application/vnd.horizon.perf"}
        if token:
            headers["Authorization"] = f"Bearer {token}"
        req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
        try:
            response = urllib.request.urlopen(req, timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, response.headers, response.read()

    def cli(*words: str) -> str:
        env = dict(os.environ, PERF_UPLOAD_URL=base + "/api/perf/captures")
        result = subprocess.run([str(args.binary), *words], env=env, text=True, capture_output=True, check=False)
        if result.returncode:
            raise AssertionError(f"CLI {words[0]} failed with exit {result.returncode}")
        return result.stdout

    def query(source: str):
        reply = json.loads(cli("query", "meta", source))
        for capture in reply.get("captures", []):
            capture["file"] = "capture"
        return reply

    try:
        status, headers, payload = request("POST", "/api/perf/captures", raw)
        assert status == 201, (status, payload)
        assert headers["Cache-Control"] == "no-store"
        uploaded = json.loads(payload)
        identity, token = uploaded["id"], uploaded["delete_token"]
        owners.append((identity, token))
        assert uploaded["shared"] is False
        status, headers, payload = request("GET", f"/api/perf/captures/{identity}")
        assert status == 200 and payload == raw
        assert headers["Content-Type"] == "application/vnd.horizon.perf"
        assert headers["Cache-Control"] == "public, max-age=31536000, immutable"
        status, headers, payload = request("GET", f"/api/perf/captures/{identity}/meta")
        assert status == 200 and headers["Cache-Control"] == "no-store"
        assert json.loads(payload)["summary"] == uploaded["summary"]
        assert request("DELETE", f"/api/perf/captures/{identity}", token="wrong")[0] == 403
        assert request("POST", f"/api/perf/captures/{identity}/share", token="wrong")[0] == 403
        assert request("POST", f"/api/perf/captures/{identity}/share", token=token)[0] == 200
        link = base + f"/perf/c/{identity}"
        assert query(str(args.fixture)) == query(link)
        assert cli("summary", str(args.fixture)).replace(args.fixture.name, "capture") == cli("summary", link).replace(identity, "capture")
        diff = json.loads(cli("diff", str(args.fixture), link, "--json"))
        assert diff["ok"] is True
        assert request("GET", f"/perf/c/{identity}")[0] == 200
        print("PASS: upload, original bytes, metadata, immutable caching, token authorization, pin and shared page")
        print("PASS: CLI summary, query and diff read the shared capture")

        assert request("POST", "/api/perf/captures", b"not a capture")[0] == 415
        newer = bytearray(gzip.decompress(raw))
        assert newer[:2] == b"\x08\x01"
        newer[1] = 2
        assert request("POST", "/api/perf/captures", gzip.compress(newer))[0] == 422
        parsed = urllib.parse.urlsplit(base)
        connection_cls = http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
        connection = connection_cls(parsed.hostname, parsed.port, timeout=30)
        connection.request("POST", "/api/perf/captures", body=b"", headers={"Content-Type": "application/vnd.horizon.perf", "Content-Length": "67108865"})
        assert connection.getresponse().status == 413
        connection.close()
        print("PASS: malformed, unsupported-format and oversized captures are refused")

        for command, shared in [("upload", False), ("share", True)]:
            reply = json.loads(cli(command, str(args.fixture)))
            owners.append((reply["id"], reply["delete_token"]))
            assert reply["shared"] is shared
            assert request("GET", f"/api/perf/captures/{reply['id']}")[2] == raw
        print("PASS: CLI upload creates a temporary capture; CLI share pins it")
    finally:
        for identity, token in owners:
            assert request("DELETE", f"/api/perf/captures/{identity}", token=token)[0] == 204
            assert request("GET", f"/api/perf/captures/{identity}")[0] == 404
    print("PASS: every test capture was deleted")


if __name__ == "__main__":
    main()
