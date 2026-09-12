#!/usr/bin/env python3
"""Validate the extended STD/API catalog schema introduced by R-3206.

The catalog is the single source of truth for the midend lowering tables, the
Rust host-call table and the governance sink/scope classification, so this
validator fails closed on any entry that cannot feed those generators:

* function entries must carry ``params``, ``returns``, ``ir_return``,
  ``returns_value`` and (for ``std.api.*``) ``rust_symbol``;
* ``ir_return`` must match the module-level IR type grammar;
* ``scope_keys`` must come from the governance scope vocabulary;
* ``sink`` must agree with the entry's ``effects`` classification.
"""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import validate_r3007_stdlib_contract as audit  # noqa: E402

SCHEMA = "spectralang.r3206_catalog_schema.v1"
CATALOG_SCHEMA = "spectralang.stdlib_catalog.v1"
CATALOG = "packages/spectra-contract/catalog/stdlib.toml"


def catalog_errors(catalog: dict[str, object]) -> tuple[list[str], dict[str, object]]:
    """Return schema errors plus a summary of the fields that were checked."""
    errors: list[str] = []
    if catalog.get("schema") != CATALOG_SCHEMA:
        errors.append(f"catalog schema must be {CATALOG_SCHEMA}")
    entries = catalog.get("entry", [])
    if not isinstance(entries, list):
        return ["catalog must declare [[entry]] tables"], {}
    paths: list[str] = []
    functions = 0
    rust_symbols = 0
    sink_entries = 0
    scoped_entries = 0
    cfg_features: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict):
            errors.append("catalog entry is not a table")
            continue
        path = str(entry.get("path", ""))
        if not path:
            errors.append("catalog entry is missing a path")
            continue
        paths.append(path)
        kind = str(entry.get("kind", "function"))
        if kind not in {"module", "type", "function"}:
            errors.append(f"{path} has an invalid kind {kind!r}")
            continue
        params = entry.get("params", [])
        if not isinstance(params, list):
            errors.append(f"{path} params must be a list")
        else:
            for param in params:
                if not isinstance(param, dict) or not param.get("name") or not param.get("ty"):
                    errors.append(f"{path} has a malformed param {param!r}")
        if entry.get("sink") not in (None, True, False):
            errors.append(f"{path} sink must be a boolean")
        if entry.get("sink") and "mutation" not in entry.get("effects", []):
            errors.append(f"{path} sink must be backed by a mutation effect")
        if entry.get("sink"):
            sink_entries += 1
        scope_keys = entry.get("scope_keys", [])
        if not isinstance(scope_keys, list):
            errors.append(f"{path} scope_keys must be a list")
        for scope_key in scope_keys:
            if scope_key not in audit.ALLOWED_SCOPE_KEYS:
                errors.append(f"{path} has an unknown scope key {scope_key!r}")
        if scope_keys:
            scoped_entries += 1
        if kind != "function":
            continue
        functions += 1
        for field_name in ("returns", "ir_return"):
            if not str(entry.get(field_name, "")).strip():
                errors.append(f"{path} is missing {field_name}")
        if "returns_value" not in entry:
            errors.append(f"{path} is missing returns_value")
        elif not isinstance(entry["returns_value"], bool):
            errors.append(f"{path} returns_value must be a boolean")
        ir_return = str(entry.get("ir_return", ""))
        if ir_return and not audit.ir_return_is_valid(ir_return):
            errors.append(f"{path} has an invalid ir_return {ir_return!r}")
        if path.startswith("std.api."):
            if not str(entry.get("rust_symbol", "")).strip():
                if path not in audit.HOST_CALL_ALIASES:
                    errors.append(f"{path} is missing rust_symbol")
            else:
                rust_symbols += 1
        cfg_feature = str(entry.get("cfg_feature", "")).strip()
        if cfg_feature:
            cfg_features.add(cfg_feature)
    if len(paths) != len(set(paths)):
        errors.append("catalog contains duplicate paths")
    summary = {
        "entry_count": len(paths),
        "function_count": functions,
        "api_rust_symbols": rust_symbols,
        "sink_entries": sink_entries,
        "scoped_entries": scoped_entries,
        "cfg_features": sorted(cfg_features),
    }
    return errors, summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", default=CATALOG)
    parser.add_argument(
        "--report",
        default="target/r3206-catalog-schema/report.json",
        help="JSON report output path, relative to the repository root",
    )
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    catalog_path = Path(args.catalog)
    catalog_path = catalog_path if catalog_path.is_absolute() else root / catalog_path
    report_path = Path(args.report)
    report_path = report_path if report_path.is_absolute() else root / report_path
    report_path.parent.mkdir(parents=True, exist_ok=True)

    errors: list[str] = []
    details: dict[str, object] = {"catalog": str(catalog_path.relative_to(root)) if catalog_path.is_relative_to(root) else str(catalog_path)}
    if not catalog_path.is_file():
        errors.append(f"catalog is missing: {catalog_path}")
    else:
        try:
            catalog = tomllib.loads(catalog_path.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError) as exc:
            errors.append(f"catalog does not parse: {exc}")
            catalog = None
        if catalog is not None:
            catalog_errors_list, summary = catalog_errors(catalog)
            errors.extend(catalog_errors_list)
            details.update(summary)

    # Planning bookkeeping: the item is only complete when the backlog and the
    # suite runner agree with the code.
    backlog_path = root / "docs/roadmap-backlog.md"
    runner_path = root / "run_tests.ps1"
    if backlog_path.is_file():
        backlog = backlog_path.read_text(encoding="utf-8")
        marker = "## R-3206 Extended Contract Catalog Schema"
        if marker in backlog:
            block = backlog.split(marker, 1)[1].split("## R-3207", 1)[0]
            if "Status: `complete`" not in block:
                errors.append("backlog R-3206 is not marked complete")
            if "validate_r3206_catalog_schema.py" not in block:
                errors.append("backlog R-3206 does not reference its validator")
        else:
            errors.append("backlog is missing the R-3206 section")
    else:
        errors.append("docs/roadmap-backlog.md is missing")
    if runner_path.is_file():
        if "validate_r3206_catalog_schema.py" not in runner_path.read_text(encoding="utf-8"):
            errors.append("run_tests.ps1 does not register validate_r3206_catalog_schema.py")
    else:
        errors.append("run_tests.ps1 is missing")

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
        print(f"R-3206 catalog schema validation failed ({len(errors)} problem(s)); report: {report_path}")
        return 1

    print("R-3206 catalog schema validation passed")
    print(json.dumps(details, sort_keys=True))
    print(f"report: {report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
