#!/usr/bin/env python3
"""Validate the superseded-partial HTTP/3/QUIC state for R-2406.

FakeToReal-17: a localhost-validated quinn+h3 transport landed, so ADR 0014
is superseded-partial (not a pure deferral). This validator pins the new
lockstep set: ADR supersession table, localhost-surface docs with limits,
the R-2422 productionization item, the R-2406 annotation, and the runner
wiring. It fails closed on any missing piece.
"""

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
        "Superseded-partial",
        "R-2422",
        "NOT MET",
        "quinn",
    ]:
        require(term in adr, f"ADR 0014 misses {term}")

    docs = read("docs/api/std-api-http3.md")
    for term in ["quinn", "ALPN", "loopback", "R-2422", "R-2406", "spectra.api.http3.server_start"]:
        require(term in docs, f"HTTP/3 docs miss {term}")
    require("std-api-http3.md" in read("docs/api/README.md"), "API README misses HTTP/3 surface")

    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}

    item = items.get("R-2406")
    require(item is not None, "R-2406 is missing from roadmap")
    require(item.get("status") == "complete", "R-2406 must stay complete as a decision")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in ["localhost-only", "R-2422", "validate_r2406_http3_decision.py"]:
        require(term.lower() in acceptance.lower(), f"R-2406 acceptance misses {term}")

    prod = items.get("R-2422")
    require(prod is not None, "R-2422 is missing from roadmap")
    require(prod.get("status") == "not_started", "R-2422 must be not_started")
    prod_acceptance = "\n".join(prod.get("acceptance", []))
    for term in ["independent peers", "BSD", "migration", "budget"]:
        require(term.lower() in prod_acceptance.lower(), f"R-2422 acceptance misses {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2406 HTTP/3 and QUIC", 1)[1].split("## R-2407", 1)[0]
    for term in ["Status: `complete`", "R-2422", "quinn", "validate_r2406_http3_decision.py"]:
        require(term.lower() in block.lower(), f"backlog R-2406 misses {term}")
    require("## R-2422 HTTP/3 Productionization" in backlog, "backlog misses R-2422")

    strategic = read("docs/production-ai-implementation-plan.md")
    require("R-2422" in strategic, "strategic plan misses R-2422")
    require("superseded-partial" in strategic, "strategic plan misses supersession")
    require("validate_r2406_http3_decision.py" in read("run_tests.ps1"), "runner does not invoke R-2406")
    print("validated R-2406 HTTP/3/QUIC superseded-partial state")


if __name__ == "__main__":
    main()
