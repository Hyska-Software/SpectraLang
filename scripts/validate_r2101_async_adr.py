#!/usr/bin/env python3
"""Validate R-2101 async/await execution model ADR coverage."""

from __future__ import annotations

import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
ADR_PATH = ROOT / "docs" / "adr" / "0010-async-execution-model.md"
ROADMAP_PATH = ROOT / "roadmap" / "roadmap.toml"
BACKLOG_PATH = ROOT / "docs" / "roadmap-backlog.md"


REQUIRED_SECTIONS = [
    "Context",
    "Decision",
    "Public Syntax Surface",
    "Logical Types",
    "Lowering Contract",
    "Scheduler Interface",
    "Structured Concurrency",
    "Cancellation",
    "Send and Sync Rules",
    "Pinning and Frame Stability",
    "Blocking and Host Calls",
    "Diagnostics",
    "Consequences",
    "Rejected Alternatives",
    "Acceptance Evidence",
]

REQUIRED_TERMS = [
    "Status: Superseded by docs/adr/0015-async-execution-model.md",
    "Roadmap item: R-2101",
    "async func",
    "async {",
    "await",
    "Task<T>",
    "Stream<T>",
    "Pin<T>",
    "stackless",
    "state-machine SSA",
    "poll(task_handle, scheduler_context) -> PollStatus",
    "Pending",
    "Ready",
    "Failed",
    "Cancelled",
    "structured concurrency",
    "cancellation propagates",
    "Send",
    "Sync",
    "epoll",
    "IOCP",
    "kqueue",
    "blocking host call",
]


def fail(message: str) -> int:
    print(f"R-2101 validation failed: {message}", file=sys.stderr)
    return 1


def require_file(path: pathlib.Path) -> str:
    if not path.exists():
        raise FileNotFoundError(path)
    return path.read_text(encoding="utf-8")


def main() -> int:
    try:
        adr = require_file(ADR_PATH)
        roadmap = require_file(ROADMAP_PATH)
        backlog = require_file(BACKLOG_PATH)
    except FileNotFoundError as exc:
        return fail(f"required file is missing: {exc}")

    for section in REQUIRED_SECTIONS:
        if f"## {section}" not in adr:
            return fail(f"missing ADR section: {section}")

    normalized = re.sub(r"\s+", " ", adr)
    for term in REQUIRED_TERMS:
        if term not in adr and term not in normalized:
            return fail(f"missing required ADR decision term: {term}")

    if 'id = "R-2101"' not in roadmap or 'status = "complete"' not in roadmap:
        return fail("roadmap.toml does not mark R-2101 complete")

    r2101_block = re.search(
        r'id = "R-2101".*?(?=\n\[\[items\]\]|\Z)',
        roadmap,
        flags=re.DOTALL,
    )
    if not r2101_block:
        return fail("roadmap.toml R-2101 block not found")
    block_text = r2101_block.group(0)
    for expected in [
        'status = "complete"',
        'docs/adr/0010-async-execution-model.md',
        "`Task<T>`",
        "`Stream<T>`",
        "`Pin`",
        "state machine SSA",
        "Send/Sync",
        "cancellation propagation",
    ]:
        if expected not in block_text:
            return fail(f"R-2101 roadmap block missing: {expected}")

    backlog_match = re.search(
        r"## R-2101 ADR: Async/Await Execution Model.*?(?=\n## R-2102|\Z)",
        backlog,
        flags=re.DOTALL,
    )
    if not backlog_match:
        return fail("backlog R-2101 section not found")
    backlog_block = backlog_match.group(0)
    for expected in [
        "Status: `complete`",
        "Acceptance Evidence",
        "docs/adr/0010-async-execution-model.md",
        "scripts/validate_r2101_async_adr.py",
    ]:
        if expected not in backlog_block:
            return fail(f"backlog R-2101 section missing: {expected}")

    print("validated R-2101 async/await execution model ADR")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
