"""Independent R-2507 gate against a real Redis 7 endpoint."""
from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import tempfile
import time
from urllib.parse import urlsplit
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def sensitive_values(secret: str | None) -> list[str]:
    if not secret:
        return []
    parsed = urlsplit(secret)
    return sorted({secret, parsed.password or ""} - {""}, key=len, reverse=True)


def scrub(value: object, secret: str | None) -> object:
    if isinstance(value, str):
        for sensitive in sensitive_values(secret):
            value = value.replace(sensitive, "<redacted>")
        return value
    if isinstance(value, list):
        return [scrub(item, secret) for item in value]
    if isinstance(value, dict):
        return {key: scrub(item, secret) for key, item in value.items()}
    return value


def run(command: list[str], env: dict[str, str], secret: str | None = None) -> dict:
    process = subprocess.run(command, cwd=ROOT, env=env, text=True, capture_output=True, timeout=240)
    return scrub({"command": command, "exit_code": process.returncode, "stdout_tail": process.stdout[-2000:], "stderr_tail": process.stderr[-2000:]}, secret)


def redis_url_parts(value: str) -> tuple[str, int]:
    without_scheme = value.split("://", 1)[-1].rsplit("@", 1)[-1]
    host_port = without_scheme.split("/", 1)[0]
    host, _, port = host_port.rpartition(":")
    return host or "127.0.0.1", int(port or "6379")


def resp(command: list[str]) -> bytes:
    payload = f"*{len(command)}\r\n" + "".join(f"${len(item.encode())}\r\n{item}\r\n" for item in command)
    return payload.encode()


def independent_probe(url: str) -> dict:
    host, port = redis_url_parts(url)
    with socket.create_connection((host, port), timeout=5) as connection:
        connection.sendall(resp(["PING"]))
        ping = connection.recv(128)
        connection.sendall(resp(["SET", "spectra:r2507:validator", "ok"]))
        set_result = connection.recv(128)
        connection.sendall(resp(["GET", "spectra:r2507:validator"]))
        get_result = connection.recv(128)
        connection.sendall(resp(["DEL", "spectra:r2507:validator"]))
        delete_result = connection.recv(128)
        connection.sendall(resp(["INFO", "server"]))
        info_result = connection.recv(8192)
    version = next(
        (line.split(b":", 1)[1].decode(errors="replace") for line in info_result.splitlines() if line.startswith(b"redis_version:")),
        "",
    )
    return {
        "ping": ping.startswith(b"+PONG"),
        "set": set_result.startswith(b"+OK"),
        "get": b"ok" in get_result,
        "delete": b":1" in delete_result,
        "version": version,
        "version_is_7": version.startswith("7."),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/debug/spectralang.exe")
    parser.add_argument("--redis-url", default=None)
    parser.add_argument("--fixture", default="tests/validation/196_redis_driver.spectra")
    parser.add_argument("--report", required=True)
    parser.add_argument(
        "--require-redis",
        action="store_true",
        help="fail instead of skipping when Redis 7 is unavailable",
    )
    args = parser.parse_args()
    url = args.redis_url or os.environ.get("SPECTRA_REDIS_URL")
    report = {"schema": "spectralang.r2507_redis.v1", "environment": {"redis_required": True, "redis_version": "7", "configured": bool(url)}, "connection": {}, "pool": {}, "commands": {}, "values": {}, "ttl": {}, "concurrency": {}, "timeouts": {}, "cancellation": {}, "shutdown": {}, "pubsub": {}, "cache_contract": {}, "tracing": {}, "security": {}, "diagnostics": {}, "failures": [], "status": "skipped_environment" if not url else "failed"}
    if not url:
        report["environment"]["reason"] = "SPECTRA_REDIS_URL is not configured; the Redis 7 CI lane is required."
        if args.require_redis:
            report["status"] = "failed"
            report["failures"].append("Redis 7 is required but SPECTRA_REDIS_URL is not configured")
    else:
        try:
            report["connection"]["independent_probe"] = independent_probe(url)
            if not all(
                report["connection"]["independent_probe"].get(key, False)
                for key in ("ping", "set", "get", "delete", "version_is_7")
            ):
                report["failures"].append("independent Redis RESP probe or Redis 7 version check failed")
        except Exception as error:
            report["failures"].append(f"Redis endpoint unavailable: {error}")
        env = os.environ.copy(); env["SPECTRA_REDIS_URL"] = url
        tests = run(["cargo", "test", "-p", "spectra-db", "--test", "redis_integration", "--", "--test-threads=1"], env, url)
        report["connection"]["rust_integration"] = tests
        if tests["exit_code"] != 0: report["failures"].append("Redis integration tests failed")
        fixture = Path(args.fixture)
        with tempfile.TemporaryDirectory(prefix="spectra-r2507-") as directory:
            generated = Path(directory) / fixture.name
            run_id = f"{os.getpid()}-{time.time_ns()}"
            fixture_source = fixture.read_text(encoding="utf-8")
            fixture_source = fixture_source.replace("__SPECTRA_REDIS_URL__", url)
            fixture_source = fixture_source.replace("__SPECTRA_REDIS_RUN_ID__", run_id)
            generated.write_text(fixture_source, encoding="utf-8")
            cli = run([str(ROOT / args.binary), "run", str(generated)], env, url)
            report["connection"]["cli_fixture"] = cli
            if cli["exit_code"] != 0: report["failures"].append("Redis Spectra fixture failed")
        report["security"]["secret_not_in_report"] = url not in json.dumps(report)
        if not report["security"]["secret_not_in_report"]: report["failures"].append("Redis URL leaked into report")
        report["status"] = "passed" if not report["failures"] else "failed"
    output = Path(args.report); output.parent.mkdir(parents=True, exist_ok=True); output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["status"] in {"passed", "skipped_environment"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
