from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 536
RUNTIME_REQUIRED_HOST_CALL_COUNT = 439


def cargo_command() -> str:
    cargo = "cargo.exe" if sys.platform.startswith("win") else "cargo"
    home = Path(os.environ.get("USERPROFILE") or os.environ.get("HOME") or "")
    candidate = home / ".cargo" / "bin" / cargo
    return str(candidate) if candidate.exists() else "cargo"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2305 validation failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_command(args: list[str]) -> None:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if completed.returncode != 0:
        fail(f"command {' '.join(args)} failed:\n{completed.stdout}")


def validate_implementation() -> None:
    manifest = read("packages/spectra-api/Cargo.toml")
    require('brotli = "8"' in manifest, "brotli dependency is not declared")
    require('flate2 = "1.1"' in manifest, "flate2 dependency is not declared")

    middleware = read("packages/spectra-api/src/middleware.rs")
    for term in [
        "enum CompressionEncoding",
        "pub struct CompressionMiddleware",
        "fn parse_quality",
        "fn negotiate",
        "CompressorWriter",
        "GzEncoder",
        "ZlibEncoder",
        "fn add_vary",
        "cache-control",
        "pub extern \"C\" fn register_compression",
        "compression_negotiates_each_encoding_and_round_trips_body",
        "compression_honors_q_values_threshold_and_http_exclusions",
    ]:
        require(term in middleware, f"middleware.rs missing {term}")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    name = "spectra.api.middleware.register_compression"
    require(name in host_calls, f"{name} missing from package host registry")
    require(name in runtime_api, f"{name} missing from runtime contract")
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})"
        in read("packages/spectra-api/src/api_tests.rs"),
        "package host-call count is not synchronized",
    )
    require(
        f"assert_eq!(required_host_call_count(), {RUNTIME_REQUIRED_HOST_CALL_COUNT})"
        in runtime_api,
        "runtime required host-call count is not synchronized",
    )


def validate_surface() -> None:
    semantic = read("compiler/src/semantic/builtin_api_services.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    require("register_compression" in semantic, "semantic surface missing register_compression")
    require(
        "std.api.middleware.register_compression" in contract,
        "contract surface missing register_compression",
    )
    require(
        "spectra.api.middleware.register_compression" in lowering,
        "lowering surface missing register_compression",
    )
    require(
        "func std.api.middleware.register_compression: func(int) returns MiddlewareHandle"
        in snapshot,
        "snapshot surface missing register_compression",
    )


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/336_api_compression.spectra")
    for term in [
        "register_compression",
        "Accept-Encoding",
        "Content-Encoding",
        "Vary",
        "br",
        "gzip",
        "deflate",
        "1000",
    ]:
        require(term in fixture, f"compression fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.middleware.register_compression"',
        'binding = "spectra.api.middleware.register_compression"',
        'fixture = "tests/validation/336_api_compression.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-middleware.md")
    for term in [
        "register_compression",
        "Accept-Encoding",
        "Content-Encoding",
        "Vary: Accept-Encoding",
        "Cache-Control",
        "336_api_compression.spectra",
    ]:
        require(term in docs, f"middleware docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2305")
    require(item is not None, "R-2305 missing from roadmap")
    require(item.get("status") == "complete", "R-2305 is not complete")
    require(item.get("owner") == "web", "R-2305 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "best supported encoding",
        "small responses",
        "Content-Encoding",
        "336_api_compression.spectra",
        "validate_r2305_compression.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2305 Response Compression", 1)[1].split(
        "## R-2306", 1
    )[0]
    for term in [
        "Status:",
        "complete",
        "336_api_compression.spectra",
        "validate_r2305_compression.py",
    ]:
        require(term in block, f"backlog R-2305 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2305_compression.py" in runner, "runner does not invoke R-2305")
    require(
        'Teste = "validate_r2305_compression"' in runner,
        "runner does not record R-2305",
    )


def validate_commands() -> None:
    cargo = cargo_command()
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    run_command(
        [cargo, "test", "-q", "-p", "spectra-api", "--lib", "middleware", "--offline"]
    )
    run_command(
        [
            cargo,
            "test",
            "-q",
            "-p",
            "spectra-compiler",
            "--test",
            "snapshot_tests",
            "--offline",
        ]
    )
    run_command([cargo, "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "check", "tests/validation/336_api_compression.spectra"])
    run_command([str(binary), "run", "tests/validation/336_api_compression.spectra"])


def main() -> None:
    validate_implementation()
    validate_surface()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2305 response compression")


if __name__ == "__main__":
    main()
