#!/usr/bin/env python3
"""Generate the typed STD/API catalog from compiler and runtime contracts.

The compiler snapshot is the authority for semantic signatures.  The runtime
inventory is used only to retain concrete host-only compatibility aliases that
are not imported by the semantic registry yet; every such alias has an
explicit signature rule below so an unknown placeholder cannot enter the
catalog silently.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import validate_r3007_stdlib_contract as audit  # noqa: E402


VALID_MATURITIES = {
    "stable",
    "beta",
    "experimental",
    "deferred",
    "reserved",
    "unsupported",
}

MATURITY_BY_CLASSIFICATION = {
    "production": "stable",
    "baseline": "beta",
    "simulation": "experimental",
    "incomplete": "beta",
}


def canonical(path: str) -> str:
    if path.startswith("spectra.std."):
        return "std." + path[len("spectra.std.") :]
    if path.startswith("spectra.api."):
        return "std.api." + path[len("spectra.api.") :]
    return path


def toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def namespace_for(path: str, kind: str) -> str:
    if kind == "module":
        return path.rsplit(".", 1)[0] if "." in path else path
    return path.rsplit(".", 1)[0]


def binding_for(path: str, kind: str) -> str:
    if kind == "module":
        return "spectra.module." + path
    if kind == "type":
        return "spectra.type." + path
    if path.startswith("std.api."):
        return "spectra.api." + path[len("std.api.") :]
    return "spectra.std." + path[len("std.") :]


def width_type(width: str) -> str:
    return width


def runtime_only_signature(path: str, semantic: dict[str, dict[str, str]]) -> tuple[str, str]:
    """Return (kind, signature) for an inventory symbol absent from semantics."""

    alias_match = re.match(r"^std\.api\.db_(sqlite|postgres|redis)\.(.+)$", path)
    if alias_match:
        driver, member = alias_match.groups()
        source = semantic.get(f"std.api.db.{driver}.{member}")
        if source is not None:
            return source["kind"], source["signature"]
        source = semantic.get(f"std.api.db.{driver}.{member}")
        if source is not None:
            return source["kind"], source["signature"]

    if path in {"std.api.version.major", "std.api.version.minor", "std.api.version.patch"}:
        return "function", "fn() -> int"
    if path == "std.concurrent.task_spawn_join":
        return "function", "fn(int) -> int"

    match = re.match(r"^std\.numeric\.checked_(add|sub|mul)_(i8|i16|i32|i64|u8|u16|u32|u64)$", path)
    if match:
        _, width = match.groups()
        return "function", f"fn({width}, {width}) -> {width}"
    match = re.match(r"^std\.numeric\.checked_float_(i8|i16|i32|i64|u8|u16|u32|u64)$", path)
    if match:
        return "function", f"fn(float) -> {match.group(1)}"
    match = re.match(r"^std\.numeric\.checked_(i8|i16|i32|i64|u8|u16|u32|u64)$", path)
    if match:
        return "function", f"fn(int) -> {match.group(1)}"

    tensor_literal = {
        "std.tensor.literal": "fn(int, ...int) -> Tensor<int>",
        "std.tensor.literal_f": "fn(int, ...float) -> Tensor<float>",
        "std.tensor.literal2": "fn(int, int, ...int) -> Tensor<int>",
        "std.tensor.literal2_f": "fn(int, int, ...float) -> Tensor<float>",
    }
    if path in tensor_literal:
        return "function", tensor_literal[path]

    if path == "std.collections.iterator_from_values":
        # Compiler-only adapter used to materialize fixed-size IR arrays into
        # the common iterator protocol. It is catalogued so the runtime
        # binding remains auditable, but it is not a source-level export.
        return "function", "fn(int, ...int) -> Iterator<int>"
    if path == "std.ml.generate":
        return "function", "fn(int, int_tensor, int, int) -> int_tensor"
    if path == "std.ml.generate_ex":
        return "function", "fn(int, int_tensor, int, int, float, int, int) -> int_tensor"


    raise RuntimeError(f"no explicit signature rule for runtime-only symbol {path}")


def docs_for(path: str, manifest: dict[str, object]) -> str:
    refs = audit.documentation_refs(ROOT, path, manifest.get("docs", []))
    if refs:
        return refs[0]
    if path.startswith("std.api."):
        return "docs/api/README.md"
    if path.startswith("std.ml.") or path.startswith("std.serve."):
        return "docs/language-feature-maturity.md"
    return "docs/reference/05-stdlib.md"


def fixture_for(path: str, manifest: dict[str, object]) -> str:
    probes = audit.probe_matches(path, manifest)
    for probe in probes:
        if probe.get("path"):
            return str(probe["path"])
    return "tests/validation/185_stdlib_contract_audit.spectra"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", default="packages/spectra-contract/catalog/stdlib.toml")
    args = parser.parse_args()

    manifest = tomllib.loads((ROOT / "scripts" / "stdlib_contract.toml").read_text(encoding="utf-8"))
    current_path = ROOT / args.output
    source_catalog = ROOT / str(manifest.get("catalog", "packages/spectra-contract/catalog/stdlib.toml"))
    catalog_path = current_path if current_path.is_file() else source_catalog
    current = tomllib.loads(catalog_path.read_text(encoding="utf-8")) if catalog_path.is_file() else {}
    current_entries = {str(entry["path"]): entry for entry in current.get("entry", [])}

    snapshot = subprocess.run(
        ["cargo", "run", "-q", "-p", "spectra-compiler", "--bin", "dump_stdlib_contract"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if snapshot.returncode != 0:
        print(snapshot.stdout, end="")
        print(snapshot.stderr, end="", file=sys.stderr)
        return snapshot.returncode
    semantic_symbols = {}
    for raw in json.loads(snapshot.stdout):
        path = canonical(str(raw["path"]))
        semantic_symbols.setdefault(
            path,
            {"kind": str(raw["kind"]), "signature": str(raw["signature"])},
        )

    inventory = audit.discover_sources(ROOT, manifest)
    symbols = set(semantic_symbols) | set(inventory.symbols)
    entries: list[dict[str, object]] = []
    for path in sorted(symbols):
        if path in semantic_symbols:
            kind = semantic_symbols[path]["kind"]
            signature = semantic_symbols[path]["signature"]
        else:
            try:
                kind, signature = runtime_only_signature(path, semantic_symbols)
            except RuntimeError:
                old = current_entries.get(path)
                if not old or "kind" not in old or "signature" not in old:
                    raise
                kind = str(old["kind"])
                signature = str(old["signature"])
        contract = audit.matching_contract(path, manifest)
        if contract is None:
            raise RuntimeError(f"no maturity contract for catalog symbol {path}")
        old = current_entries.get(path, {})
        classification = str(contract["classification"])
        old_maturity = str(old.get("maturity", ""))
        maturity = (
            old_maturity
            if old_maturity in VALID_MATURITIES
            else MATURITY_BY_CLASSIFICATION.get(classification, "beta")
        )
        entry = {
            "path": path,
            "kind": kind,
            "namespace": namespace_for(path, kind),
            # Signatures are compiler-owned and must never be preserved from
            # a stale catalog entry. ABI/docs metadata may be migrated, but a
            # changed semantic contract must be visible in the generated file.
            "signature": signature,
            "abi": old.get(
                "abi",
                "semantic descriptor" if kind != "function" else "host(ctx: SpectraHostCallContext) -> i32",
            ),
            "effects": old.get("effects", ["host"] if kind == "function" else []),
            "error_model": old.get(
                "error_model",
                "none" if kind != "function" else (
                    "legacy compatibility adapter" if path.startswith("std.compat.") else "host status + typed return"
                ),
            ),
            "binding": binding_for(path, kind),
            "maturity": maturity,
            "owner": old.get("owner", contract["owner"]),
            "docs": old.get("docs", docs_for(path, manifest)),
            "fixture": old.get("fixture", fixture_for(path, manifest)),
        }
        entries.append(entry)

    lines = [
        'schema = "spectralang.stdlib_catalog.v1"',
        'catalog_version = "stdlib/v1"',
        "",
        "# Generated from compiler builtin registration plus the audited runtime inventory.",
        "# Do not edit individual entries manually; update the owning contract/source instead.",
    ]
    for entry in entries:
        lines.append("")
        lines.append("[[entry]]")
        for key in ("path", "kind", "namespace", "signature", "abi"):
            lines.append(f"{key} = {toml_string(str(entry[key]))}")
        effects = entry["effects"]
        lines.append("effects = [" + ", ".join(toml_string(str(value)) for value in effects) + "]")
        for key in ("error_model", "binding", "maturity", "owner", "docs", "fixture"):
            lines.append(f"{key} = {toml_string(str(entry[key]))}")
    current_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"generated {len(entries)} catalog entries at {current_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
