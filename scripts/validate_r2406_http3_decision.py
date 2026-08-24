#!/usr/bin/env python3
"""Validate the deferred HTTP/3/QUIC decision for R-2406."""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print(f"R-2406 validation failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def main() -> None:
    adr = read("docs/adr/0014-http3-quic-decision.md")
    for term in [
        "Status: Accepted",
        "Roadmap item: R-2406",
        "Defer HTTP/3 and QUIC implementation",
        "2026-11-30",
        "Linux, Windows, macOS, and BSD",
        "connection migration",
        "Task<T>`/`Stream<T>",
    ]:
        require(term in adr, f"ADR 0014 misses {term}")

    docs = read("docs/api/std-api-http3.md")
    for term in ["deferred", "ADR 0014", "2026-11-30", "HTTP/1.1", "HTTP/2"]:
        require(term in docs, f"HTTP/3 docs miss {term}")
    require("std-api-http3.md" in read("docs/api/README.md"), "API README misses HTTP/3 decision")

    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2406")
    require(item is not None, "R-2406 is missing from roadmap")
    require(item.get("status") == "complete", "R-2406 must be complete as a decision")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in ["stable Rust QUIC", "HTTP/2", "2026-11-30", "ADR 0014", "validate_r2406_http3_decision.py"]:
        require(term.lower() in acceptance.lower(), f"R-2406 acceptance misses {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2406 HTTP/3 and QUIC", 1)[1].split("## R-2407", 1)[0]
    for term in ["Status: `complete`", "scope decision", "ADR 0014", "2026-11-30", "validate_r2406_http3_decision.py"]:
        require(term.lower() in block.lower(), f"backlog R-2406 misses {term}")
    strategic = read("docs/production-ai-implementation-plan.md")
    require("R-2406` HTTP/3 and QUIC (complete as a scope decision;" in strategic, "strategic plan misses R-2406")
    require("validate_r2406_http3_decision.py" in read("run_tests.ps1"), "runner does not invoke R-2406")
    print("validated R-2406 HTTP/3/QUIC decision")


if __name__ == "__main__":
    main()
