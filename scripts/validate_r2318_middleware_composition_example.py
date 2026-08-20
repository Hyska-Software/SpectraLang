#!/usr/bin/env python3
"""Validate the R-2318 production middleware composition example."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print(f"R-2318 validation failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def cargo_cmd() -> str:
    configured = os.environ.get("CARGO")
    if configured:
        return configured
    return shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo.exe")


def run_command(args: list[str], timeout: int = 120) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
        check=False,
    )
    if completed.returncode != 0:
        fail(f"command {' '.join(args)} failed:\n{completed.stdout}")
    return completed.stdout


def validate_source() -> None:
    example = read("examples/api/03_middleware_composition.spectra")
    for term in [
        "module middleware_composition_api",
        "register_logging",
        "register_security_headers",
        "security_headers_route",
        "cors_middleware",
        "register_compression",
        "register_rate_limit",
        '"route"',
        "logging -> security headers -> CORS ->",
        "compression -> rate limit",
        "Content-Encoding",
        "status=429",
        "Access-Control-Allow-Origin",
        "default-src 'none'",
        "default-src https://api.example",
        "method_options()",
        "trace_short_circuited",
    ]:
        require(term in example, f"composition example missing {term}")

    cors = read("packages/spectra-api/src/cors.rs")
    require("fn add_vary" in cors, "CORS Vary merge helper is missing")
    require("actual_cors_merges_vary_with_existing_compression_dimension" in cors, "CORS Vary regression is missing")


def validate_docs() -> None:
    middleware = read("docs/api/std-api-middleware.md")
    for term in [
        "examples/api/03_middleware_composition.spectra",
        "structured logging -> security headers -> CORS -> compression -> rate limit",
        "Vary: Accept-Encoding",
        "longest-prefix",
        "route-scoped sliding-window",
        "validate_r2318_middleware_composition_example.py",
    ]:
        require(term in middleware, f"middleware docs missing {term}")
    cors = read("docs/api/std-api-cors.md")
    require("appends" in cors and "instead of replacing it" in cors, "CORS docs do not describe Vary merging")
    index = read("docs/api/README.md")
    require("03_middleware_composition.spectra" in index, "API docs index misses R-2318 example")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2318")
    require(item is not None, "R-2318 missing from roadmap")
    require(item.get("status") == "complete", "R-2318 is not complete")
    require(item.get("owner") == "ecosystem", "R-2318 owner changed")
    require(
        item.get("dependencies") == ["R-2302", "R-2303", "R-2304", "R-2306"],
        "R-2318 dependencies changed",
    )
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "03_middleware_composition.spectra",
        "middleware order",
        "per-route configuration",
        "Vary",
        "validate_r2318_middleware_composition_example.py",
    ]:
        require(term in acceptance, f"R-2318 roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2318 API Example: Middleware Composition", 1)[1].split(
        "## R-2401", 1
    )[0]
    for term in [
        "Status:",
        "complete",
        "03_middleware_composition.spectra",
        "logging",
        "security headers",
        "rate limit",
        "validate_r2318_middleware_composition_example.py",
    ]:
        require(term in block, f"backlog R-2318 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2318` API example: middleware composition (complete;" in plan,
        "implementation plan must mark R-2318 complete",
    )

    runner = read("run_tests.ps1")
    require("validate_r2318_middleware_composition_example.py" in runner, "runner does not invoke R-2318")
    require(
        'Teste = "validate_r2318_middleware_composition_example"' in runner,
        "runner does not record R-2318",
    )


def main() -> None:
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    validate_source()
    validate_docs()
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-api", "--lib", "cors", "--offline"])
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-api", "--lib", "middleware", "--offline"])
    run_command([cargo_cmd(), "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "compile", "examples/api/03_middleware_composition.spectra"])
    run_command([str(binary), "run", "examples/api/03_middleware_composition.spectra"])
    validate_planning()
    print("validated R-2318 middleware composition example")


if __name__ == "__main__":
    main()
