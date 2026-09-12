#!/usr/bin/env python3
"""Generate the agent capability reference from the contract catalog (R-3215).

The catalog (``packages/spectra-contract/catalog/stdlib.toml``) is the single
source of truth for host-call names and scope keys.  This generator renders the
grant vocabulary the compiler validates into ``docs/agent-platform.md`` between
stable markers; ``--check`` fails when the checked-in block drifts from the
catalog, so documentation cannot silently diverge from what compiles.

Usage:
    python scripts/generate_capability_reference.py
    python scripts/generate_capability_reference.py --check
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CATALOG = ROOT / "packages" / "spectra-contract" / "catalog" / "stdlib.toml"
DOC = ROOT / "docs" / "agent-platform.md"

BEGIN_MARKER = "<!-- BEGIN GENERATED CAPABILITY REFERENCE -->"
END_MARKER = "<!-- END GENERATED CAPABILITY REFERENCE -->"

PREAMBLE = """# Agent Platform

Runtime and compile-time governance for `std.agent` runs.  The phased design and
production intent live in [`docs/agent-platform-plan.md`](agent-platform-plan.md);
the capability security model — the vocabulary, the single dispatch enforcement
seam and the guarantees a run provides — is recorded in
[`docs/adr/0016-agent-capability-enforcement.md`](adr/0016-agent-capability-enforcement.md).

The reference below is generated from the contract catalog by
`scripts/generate_capability_reference.py`; run it with `--check` in CI to detect
drift.
"""


def host_calls(catalog: dict) -> list[dict]:
    """Every catalog entry the runtime dispatches as a host call.

    The catalog's ``effects`` classify the domain (``filesystem``,
    ``network``, ...); the ABI identifies the dispatch kind.
    """
    return [
        entry
        for entry in catalog["entry"]
        if entry.get("abi", "").startswith("host(")
    ]


def namespace_of(binding: str) -> str:
    """`spectra.std.fs.fs_read` -> `spectra.std.fs` (the grant form)."""
    head, separator, _ = binding.rpartition(".")
    return head if separator else binding


def render_block() -> str:
    catalog = tomllib.loads(CATALOG.read_text(encoding="utf-8"))
    calls = host_calls(catalog)

    namespaces: dict[str, int] = {}
    for entry in calls:
        namespace = namespace_of(entry["binding"])
        namespaces[namespace] = namespaces.get(namespace, 0) + 1

    scoped = sorted(
        (entry["binding"], entry.get("scope_keys", []))
        for entry in calls
        if entry.get("scope_keys")
    )

    lines: list[str] = [
        BEGIN_MARKER,
        "",
        "### Grant forms",
        "",
        "`AgentSpec.allow` grants are validated at compile time (R-3215) against the",
        "contract catalog — the same data the runtime dispatches, and the same names",
        "`surface --json` reports.  A grant is one of:",
        "",
        "- a namespace prefix, such as `spectra.std.fs`, which grants every host call",
        "  registered beneath it;",
        "- a full host call, such as `spectra.std.fs.fs_read`;",
        "- a scoped host call, such as `spectra.api.client.request:host=api.example.com`,",
        "  accepted only when the catalog declares the scope key for that host call.",
        "",
        "Grants name the runtime host call (`spectra.…`), which is what the dispatch",
        "seam evaluates.  The catalog path (`std.fs.fs_read`) is not a grant form;",
        "writing it fails `E3201`, and the diagnostic suggests the runtime name.",
        "",
        "### Namespaces",
        "",
        f"{len(namespaces)} namespace grants cover {len(calls)} host calls.",
        "",
        "| Namespace grant | Host calls |",
        "| --- | --- |",
    ]
    for namespace in sorted(namespaces):
        lines.append(f"| `{namespace}` | {namespaces[namespace]} |")

    lines.extend(
        [
            "",
            "### Scope keys",
            "",
            "A scoped grant is accepted only where the host call declares the key",
            "(catalog `scope_keys`).",
            "",
            "| Host call | Scope keys |",
            "| --- | --- |",
        ]
    )
    for binding, keys in scoped:
        rendered = ", ".join(f"`{key}`" for key in keys)
        lines.append(f"| `{binding}` | {rendered} |")

    lines.extend(["", END_MARKER, ""])
    return "\n".join(lines)


def expected_document() -> str:
    if DOC.exists():
        current = DOC.read_text(encoding="utf-8")
        if BEGIN_MARKER in current and END_MARKER in current:
            head = current.split(BEGIN_MARKER, 1)[0]
            tail = current.split(END_MARKER, 1)[1].lstrip("\n")
            return f"{head}{render_block()}{tail}"
    return f"{PREAMBLE}\n{render_block()}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify docs/agent-platform.md matches the catalog instead of writing it",
    )
    args = parser.parse_args()

    expected = expected_document()
    if args.check:
        if not DOC.exists():
            print(
                "capability reference check failed: docs/agent-platform.md is missing",
                file=sys.stderr,
            )
            return 1
        current = DOC.read_text(encoding="utf-8")
        if current != expected:
            print(
                "capability reference drift: run python scripts/generate_capability_reference.py",
                file=sys.stderr,
            )
            return 1
        print("capability reference matches the catalog")
        return 0

    if not DOC.exists():
        DOC.parent.mkdir(parents=True, exist_ok=True)
    DOC.write_text(expected, encoding="utf-8", newline="\n")
    print(f"wrote {DOC.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
