#!/usr/bin/env python3
"""Validate the machine-readable stable-core maturity contract.

The script deliberately validates rather than silently rewriting Markdown. A
future renderer can use the same schema, but a hand-edited projection must not
become a second source of truth.
"""

from __future__ import annotations

import argparse
import json
import re
import tomllib
from pathlib import Path


REQUIRED_FEATURES = {
    "core.class": "reserved",
    "core.static": "stable",
    "core.exact_width": "stable",
    "core.arrays": "beta",
    "core.iterators": "beta",
    "std.collections": "beta",
    "std.option_result": "beta",
    "core.async": "beta",
}


def load_contract(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def validate(root: Path, contract_path: Path) -> tuple[list[str], dict]:
    errors: list[str] = []
    contract = load_contract(contract_path)
    if contract.get("schema") != "spectralang.language_stability.v1":
        errors.append("unexpected language stability contract schema")

    allowed = set(contract.get("statuses", {}).get("allowed", []))
    features = contract.get("feature", [])
    ids = [feature.get("id") for feature in features]
    duplicates = sorted({feature_id for feature_id in ids if ids.count(feature_id) > 1})
    if duplicates:
        errors.append(f"duplicate feature ids: {duplicates}")

    by_id = {feature.get("id"): feature for feature in features}
    for feature_id, expected_status in REQUIRED_FEATURES.items():
        feature = by_id.get(feature_id)
        if feature is None:
            errors.append(f"missing required feature: {feature_id}")
            continue
        status = feature.get("status")
        if status not in allowed:
            errors.append(f"{feature_id} uses unknown status {status!r}")
        if status != expected_status:
            errors.append(f"{feature_id} status drift: {status!r} != {expected_status!r}")
        for field in ("name", "owner"):
            if not str(feature.get(field, "")).strip():
                errors.append(f"{feature_id} is missing {field}")

    policy_path = root / "docs" / "language-feature-maturity.md"
    policy = policy_path.read_text(encoding="utf-8")
    beta = re.search(r"^### Beta\s*$([\s\S]*?)(?=^### |\Z)", policy, re.MULTILINE)
    deferred = re.search(r"^### Deferred\s*$([\s\S]*?)(?=^### |\Z)", policy, re.MULTILINE)
    beta_text = beta.group(1) if beta else ""
    deferred_text = deferred.group(1) if deferred else ""
    if re.search(r"(?mi)^\s*-\s*class syntax footprint\s*$", beta_text):
        errors.append("class remains advertised as beta")
    if "class" not in deferred_text.lower() and "reserved" not in policy.lower():
        errors.append("class reservation is not documented")

    report = {
        "schema": "spectralang.language_stability.validation.v1",
        "contract": str(contract_path.relative_to(root)).replace("\\", "/"),
        "feature_count": len(features),
        "statuses": {
            status: sum(feature.get("status") == status for feature in features)
            for status in sorted(allowed)
        },
        "errors": errors,
        "status": "passed" if not errors else "failed",
    }
    return errors, report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--contract", default="scripts/language_stability_contract.toml")
    parser.add_argument("--report", default=None)
    args = parser.parse_args()
    root = Path(args.root).resolve()
    contract_path = (root / args.contract).resolve()
    errors, report = validate(root, contract_path)
    if args.report:
        report_path = (root / args.report).resolve()
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
