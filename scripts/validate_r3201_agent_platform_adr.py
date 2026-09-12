#!/usr/bin/env python3
"""Validate the R-3201 agent-platform ADRs, fast-path invariant and tracker entry."""

from __future__ import annotations

import argparse
import json
import tomllib
from pathlib import Path

SCHEMA = "spectralang.agent_platform.r3201.v1"

ADR_FILES = (
    "docs/adr/0016-agent-capability-enforcement.md",
    "docs/adr/0017-agent-surface-generation.md",
    "docs/adr/0018-agent-run-contract.md",
    "docs/adr/0019-agent-tool-dispatch.md",
)

ADR_REQUIRED_SECTIONS = ("## Context", "## Decision", "## Rationale", "## Consequences")

INVARIANT_TEST_NAME = "fast_host_call_effect_namespace"

RUNTIME_SOURCES = ("runtime/src/abi.rs", "runtime/src/ffi_tests.rs")

ERROR_REFERENCE = "docs/diagnostics/error-code-reference.md"
ERROR_FAMILY_ROW = "| `E3201-E3209` | semantic | Phase 32 agent platform: capability vocabulary and tool declarations |"

ROADMAP = "roadmap/roadmap.toml"
PHASE_ID = "phase_32"
ITEM_ID = "R-3201"


def require(condition: bool, errors: list[str], message: str) -> bool:
    """Record a failure when ``condition`` is false and return the condition."""
    if not condition:
        errors.append(message)
    return condition


def fail(errors: list[str], message: str) -> None:
    """Record an unconditional failure."""
    errors.append(message)


def validate_adrs(root: Path, errors: list[str], details: dict[str, object]) -> None:
    accepted: list[str] = []
    for offset, relative in enumerate(ADR_FILES, start=16):
        path = root / relative
        before = len(errors)
        if not require(path.is_file(), errors, f"missing ADR: {relative}"):
            continue
        text = path.read_text(encoding="utf-8")
        require(
            "Status: Accepted" in text,
            errors,
            f"{relative} does not declare 'Status: Accepted'",
        )
        require(
            text.startswith(f"# ADR {offset:04d}: "),
            errors,
            f"{relative} does not start with its ADR number and title",
        )
        for section in ADR_REQUIRED_SECTIONS:
            require(section in text, errors, f"{relative} is missing section '{section}'")
        if len(errors) == before:
            accepted.append(relative)
    details["adrs_accepted"] = accepted
    details["adr_count"] = len(ADR_FILES)


def validate_invariant(root: Path, errors: list[str], details: dict[str, object]) -> None:
    token = f"fn {INVARIANT_TEST_NAME}"
    found: list[str] = []
    for relative in RUNTIME_SOURCES:
        path = root / relative
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8")
        if token not in text:
            continue
        found.append(relative)
        require("#[test]" in text, errors, f"{relative} has no #[test] attribute")
        require(
            "FastHostCall::ALL" in text,
            errors,
            f"{relative} does not walk FastHostCall::ALL",
        )
        require(
            "FastHostCall::COUNT" in text,
            errors,
            f"{relative} does not assert against FastHostCall::COUNT",
        )
        require(
            "spectra.agent." in text,
            errors,
            f"{relative} does not classify the spectra.agent.* namespace",
        )
    if not require(
        len(found) > 0,
        errors,
        f"invariant test '{INVARIANT_TEST_NAME}' not found in runtime sources",
    ):
        return
    details["invariant_test"] = INVARIANT_TEST_NAME
    details["invariant_sources"] = found


def validate_tracker(root: Path, errors: list[str], details: dict[str, object]) -> None:
    path = root / ROADMAP
    if not require(path.is_file(), errors, f"missing roadmap tracker: {ROADMAP}"):
        return
    try:
        tracker = tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as error:
        fail(errors, f"{ROADMAP} does not parse: {error}")
        return

    phases = tracker.get("phases")
    if not require(isinstance(phases, list), errors, f"{ROADMAP} has no [[phases]] table"):
        return
    phase_present = any(
        isinstance(phase, dict) and phase.get("id") == PHASE_ID for phase in phases
    )
    require(phase_present, errors, f"{ROADMAP} does not register phase '{PHASE_ID}'")
    details["phase_registered"] = phase_present

    items = tracker.get("items")
    if not require(isinstance(items, list), errors, f"{ROADMAP} has no [[items]] table"):
        return
    item = next(
        (
            entry
            for entry in items
            if isinstance(entry, dict) and entry.get("id") == ITEM_ID
        ),
        None,
    )
    if not require(item is not None, errors, f"{ROADMAP} does not register item '{ITEM_ID}'"):
        return
    require(
        item.get("phase") == PHASE_ID,
        errors,
        f"{ITEM_ID} is not registered under '{PHASE_ID}'",
    )
    details["item_registered"] = item.get("id") == ITEM_ID
    details["item_phase"] = item.get("phase")


def validate_error_family(root: Path, errors: list[str], details: dict[str, object]) -> None:
    path = root / ERROR_REFERENCE
    if not require(path.is_file(), errors, f"missing diagnostics reference: {ERROR_REFERENCE}"):
        return
    text = path.read_text(encoding="utf-8")
    present = ERROR_FAMILY_ROW in text
    require(present, errors, f"{ERROR_REFERENCE} is missing the E3201-E3209 family row")
    details["error_family_row"] = present


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--report",
        default="target/r3201-agent-platform/report.json",
        help="JSON report output path, relative to the repository root",
    )
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    report_path = (root / args.report).resolve()
    report_path.parent.mkdir(parents=True, exist_ok=True)

    errors: list[str] = []
    details: dict[str, object] = {}

    validate_adrs(root, errors, details)
    validate_invariant(root, errors, details)
    validate_tracker(root, errors, details)
    validate_error_family(root, errors, details)

    report: dict[str, object] = {
        "schema": SCHEMA,
        "status": "passed" if not errors else "failed",
        **details,
        "failures": errors,
    }
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    if errors:
        for error in errors:
            print(f"error: {error}")
        print(f"R-3201 validation failed ({len(errors)} problem(s)); report: {report_path}")
        return 1

    print("R-3201 agent platform ADR validation passed")
    print(f"report: {report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
