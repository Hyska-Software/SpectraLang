"""Required release gate for the stable language/STD contract.

Development validators may report ``skipped_environment`` when a service is not
configured.  This aggregator deliberately runs the certification lane with
required flags, records each command and capability, and fails closed when any
gate is absent, skipped, timed out, or failed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = "spectralang.stability.release.v1"


def secret_values() -> list[str]:
    values: list[str] = []
    for key in ("SPECTRA_POSTGRES_URL", "SPECTRA_REDIS_URL", "SPECTRA_TLS_EXTERNAL_URL"):
        value = os.environ.get(key)
        if value:
            values.append(value)
            if "@" in value:
                values.append(value.rsplit("@", 1)[0].split(":")[-1])
    return sorted(set(filter(None, values)), key=len, reverse=True)


def scrub(text: str) -> str:
    for secret in secret_values():
        text = text.replace(secret, "<redacted>")
    return text


def tail(value: object) -> str:
    """Normalize subprocess tails, including bytes returned by a timeout."""
    if isinstance(value, bytes):
        return value.decode(errors="replace")[-4000:]
    return str(value or "")[-4000:]


def run_gate(gate_id: str, command: list[str], timeout: int) -> dict[str, object]:
    started = time.monotonic()
    try:
        process = subprocess.run(
            command,
            cwd=ROOT,
            env=os.environ.copy(),
            text=True,
            capture_output=True,
            timeout=timeout,
        )
        output = f"{process.stdout}\n{process.stderr}"
        if process.returncode != 0:
            status = "failed"
        elif "skipped_environment" in output:
            status = "skipped_environment"
        else:
            status = "passed"
        return {
            "id": gate_id,
            "command": command,
            "status": status,
            "exit_code": process.returncode,
            "duration_seconds": round(time.monotonic() - started, 3),
            "stdout_tail": scrub(tail(process.stdout)),
            "stderr_tail": scrub(tail(process.stderr)),
        }
    except subprocess.TimeoutExpired as error:
        return {
            "id": gate_id,
            "command": command,
            "status": "failed",
            "exit_code": 124,
            "duration_seconds": round(time.monotonic() - started, 3),
            "timed_out": True,
            "stdout_tail": scrub(tail(error.stdout)),
            "stderr_tail": scrub(tail(error.stderr)),
        }


def release_decision(results: list[dict[str, object]], required: bool) -> dict[str, object]:
    failures = [result["id"] for result in results if result["status"] == "failed"]
    skips = [result["id"] for result in results if result["status"] == "skipped_environment"]
    if failures:
        status = "failed"
    elif skips:
        status = "failed" if required else "skipped_environment"
    else:
        status = "passed"
    return {
        "status": status,
        "release_certifying": status == "passed",
        "failures": failures,
        "skipped_environment": skips,
    }


def binary_hash(path: Path) -> str | None:
    if not path.exists():
        return None
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git_revision() -> str | None:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, capture_output=True
    )
    return result.stdout.strip() if result.returncode == 0 else None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/debug/spectralang.exe")
    parser.add_argument("--report", default="target/stability/release-report.json")
    parser.add_argument("--markdown", default="target/stability/release-report.md")
    parser.add_argument(
        "--required",
        action="store_true",
        help="require every external capability; missing services fail closed",
    )
    parser.add_argument(
        "--postgres-version-probe-docker-container",
        default=os.environ.get("SPECTRA_POSTGRES_VERSION_PROBE_DOCKER_CONTAINER"),
        help="use psql inside this container for the independent PostgreSQL version probe",
    )
    args = parser.parse_args()

    binary = Path(args.binary)
    postgres_command = ["python", "scripts/validate_r2505_postgres.py", "--binary", str(binary), "--report", "target/stability/postgres-release.json"]
    redis_command = ["python", "scripts/validate_r2507_redis.py", "--binary", str(binary), "--report", "target/stability/redis-release.json"]
    tls_command = ["python", "scripts/validate_r2207_tls_rustls.py"]
    exact_width_command = [
        "python",
        "scripts/validate_r2901_exact_width.py",
        "--binary",
        str(binary),
        "--fixture",
        "tests/validation/189_exact_width_numeric_semantics.spectra",
        "--report",
        "target/stability/exact-width-release.json",
    ]
    if args.required:
        postgres_command.append("--require-database")
        redis_command.append("--require-redis")
        tls_command.append("--require-external")
        exact_width_command.append("--require-c-abi")
    if args.postgres_version_probe_docker_container:
        postgres_command.extend(
            [
                "--version-probe-docker-container",
                args.postgres_version_probe_docker_container,
            ]
        )

    stdlib_contract_command = [
        "python",
        "scripts/validate_r3007_stdlib_contract.py",
        "--binary",
        str(binary),
        "--timeout-seconds",
        "180",
        "--report",
        "target/stability/r3007-release.json",
    ]
    if args.required:
        stdlib_contract_command.append("--require-catalog")

    gates: list[tuple[str, list[str], int]] = [
        ("cargo-fmt", ["cargo", "fmt", "--all", "--", "--check"], 180),
        ("cargo-clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"], 900),
        ("cargo-workspace-tests", ["cargo", "test", "--workspace", "--all-targets", "--no-fail-fast"], 1200),
        (
            "module-boundaries",
            [
                "python",
                "scripts/validate_module_boundaries.py",
                "--strict",
                "--report",
                "target/stability/module-boundaries.json",
            ],
            120,
        ),
        (
            "static-cross-module-aot",
            [
                "python",
                "scripts/validate_stability_static.py",
                "--binary",
                str(binary),
                "--report",
                "target/stability/static-aot-release.json",
            ],
            300,
        ),
        ("exact-width", exact_width_command, 300),
        (
            "handle-ownership",
            [
                "python",
                "scripts/validate_handle_ownership.py",
                "--binary",
                str(binary),
                "--report",
                "target/stability/handle-ownership-release.json",
            ],
            360,
        ),
        (
            "typed-collections-iterator",
            [
                "python",
                "scripts/validate_stability_collections.py",
                "--binary",
                str(binary),
                "--report",
                "target/stability/collections-release.json",
            ],
            360,
        ),
        (
            "reactor-real-io",
            ["python", "scripts/validate_r2104_reactor.py"],
            420,
        ),
        ("stdlib-contract", stdlib_contract_command, 420),
        ("stdlib-bug-hunt", ["python", "scripts/validate_stdlib_core_bug_hunt.py", "--binary", str(binary)], 180),
        ("base-regression", ["python", "scripts/validate_r2003_base_regression_audit.py", "--binary", str(binary)], 180),
        ("language-guide", ["python", "scripts/validate_language_guide.py", "--binary", str(binary)], 180),
        ("async-stdlib", ["python", "scripts/validate_r2107_async_stdlib.py"], 180),
        ("postgresql-16", postgres_command, 900),
        ("redis-7", redis_command, 900),
        ("tls-external", tls_command, 300),
        ("otlp-collector", ["python", "scripts/validate_r2701_tracing.py", "--binary", str(binary), "--fixture", "tests/validation/193_opentelemetry_tracing.spectra", "--report", "target/stability/tracing-release.json"], 300),
    ]

    results = [run_gate(gate_id, command, timeout) for gate_id, command, timeout in gates]
    # The TLS validator can prove the local rustls surface without an external
    # endpoint.  That is useful in development, but it is not certification.
    if not args.required and not os.environ.get("SPECTRA_TLS_EXTERNAL_URL"):
        for result in results:
            if result["id"] == "tls-external" and result["status"] == "passed":
                result["status"] = "skipped_environment"
                result["skip_reason"] = "SPECTRA_TLS_EXTERNAL_URL is not configured"
    decision = release_decision(results, args.required)
    report = {
        "schema": SCHEMA,
        "required": args.required,
        **decision,
        "binary": str(binary),
        "binary_sha256": binary_hash(ROOT / binary if not binary.is_absolute() else binary),
        "git_revision": git_revision(),
        "environment": {
            "platform": platform.platform(),
            "python": platform.python_version(),
            "postgresql_configured": bool(os.environ.get("SPECTRA_POSTGRES_URL")),
            "redis_configured": bool(os.environ.get("SPECTRA_REDIS_URL")),
            "tls_external_configured": bool(os.environ.get("SPECTRA_TLS_EXTERNAL_URL")),
        },
        "gates": results,
    }
    report_path = ROOT / args.report
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    markdown = [
        "# SpectraLang stability release report",
        "",
        f"- Status: `{report['status']}`",
        f"- Release certifying: `{report['release_certifying']}`",
        f"- Binary SHA-256: `{report['binary_sha256'] or 'unavailable'}`",
        f"- Git revision: `{report['git_revision'] or 'unavailable'}`",
        "",
        "| Gate | Status | Exit | Duration (s) |",
        "| --- | --- | ---: | ---: |",
    ]
    for result in results:
        markdown.append(
            f"| `{result['id']}` | `{result['status']}` | {result['exit_code']} | {result['duration_seconds']} |"
        )
    if decision["failures"]:
        markdown.extend(["", "Required failures: " + ", ".join(f"`{item}`" for item in decision["failures"])])
    if decision["skipped_environment"]:
        markdown.extend(["", "Environment skips: " + ", ".join(f"`{item}`" for item in decision["skipped_environment"])])
    markdown_path = ROOT / args.markdown
    markdown_path.parent.mkdir(parents=True, exist_ok=True)
    markdown_path.write_text("\n".join(markdown) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "failures": decision["failures"], "skipped_environment": decision["skipped_environment"]}, indent=2))
    return 0 if report["status"] in {"passed", "skipped_environment"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
