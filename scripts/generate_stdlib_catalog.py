#!/usr/bin/env python3
"""Generate the typed STD/API catalog from compiler and runtime contracts.

The compiler snapshot is the authority for semantic signatures.  The runtime
inventory is used only to retain concrete host-only compatibility aliases that
are not imported by the semantic registry yet; every such alias has an
explicit signature rule below so an unknown placeholder cannot enter the
catalog silently.

Extended-field provenance (R-3206)
----------------------------------
* ``params``/``returns`` split the compiler-owned semantic signature.  Semantic
  signatures are anonymous, so parameters are named ``arg0``..``argN`` and a
  variadic parameter keeps its ``...`` prefix in ``ty``.
* ``ir_return``/``returns_value`` come from the hand-written midend host
  descriptor for the entry (``midend/src/lowering_std_*.rs``), rendered with the
  catalog IR type grammar (``int``, ``void``, ``Result<string>``,
  ``Tensor<float,rank=1>``, ``ExactInt<i8>``, ...).  Entries the compiler emits
  without a table descriptor (JSON derive helpers, version probes, tensor
  literals) fall back to the signature return type mapped onto that grammar.
* ``rust_symbol``/``cfg_feature`` come from ``HostCallSpec`` entries in
  ``packages/spectra-api/src/host_calls.rs``.  Only ``std.api.*`` functions own a
  host-call symbol; compiler aliases (``std.api.routing.router``) own no
  ``HostCallSpec`` and instead take the sibling host binding recorded by the
  lowering table, while every other function is registered by the runtime crate
  and intentionally carries no ``rust_symbol``.
* ``sink`` is the write-side effect classification: true when the entry's
  ``effects`` contain ``mutation``.
* ``scope_keys`` come from the explicit override table below (empty by default),
  keeping governance scope predicates explicit and reviewable.
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

# Host descriptor tables that pin a function's IR return type.  The first tuple
# element selects the catalog namespace (`api` -> `std.api.<module>`, otherwise
# `<module>` is a real std namespace).
LOWERING_TABLES = (
    ("api", "midend/src/lowering_std_api.rs"),
    ("std", "midend/src/lowering_std_host_math_io_error.rs"),
    ("std", "midend/src/lowering_std_host_collections_string.rs"),
    ("std", "midend/src/lowering_std_host_fs_env_result.rs"),
    ("std", "midend/src/lowering_std_host_convert_time.rs"),
    ("std", "midend/src/lowering_std_host_numeric.rs"),
    ("std", "midend/src/lowering_std_host_tensor_ml.rs"),
)
HOST_CALLS_SOURCE = "packages/spectra-api/src/host_calls.rs"
HOST_CALLS_PREFIX = "spectra.api."

# `("module", "function") =>` arms inside the lowering tables.
LOWERING_ARM_RE = re.compile(r'\(\s*"([a-z0-9_.]+)"\s*,\s*"([a-z0-9_]+)"\s*\)\s*=>')
# `HostCallSpec { name: "spectra.api.x", function: path::to::symbol },` plus the
# `#[cfg(feature = "...")]` that gates http3-only entries.
HOST_CALL_RE = re.compile(
    r'(?:#\[cfg\(feature\s*=\s*"([^"]+)"\)\]\s*)?'
    r'HostCallSpec\s*\{\s*name:\s*"([^"]+)"\s*,\s*function:\s*([A-Za-z_][A-Za-z0-9_:]*)\s*,?\s*\}'
)
HOST_DESCRIPTOR_HELPERS = {
    "host_int": ("int", True),
    "host_float": ("float", True),
    "host_bool": ("bool", True),
    "host_string": ("string", True),
    "host_void": ("void", False),
    "host_task_int": ("Task<int>", True),
    "host_task_bool": ("Task<bool>", True),
    "host_task_string": ("Task<string>", True),
    "host_tensor_rank0": ("Tensor<float,rank=0>", True),
    "host_tensor_dynamic": ("Tensor<float>", True),
}
IR_SCALAR_TYPES = {
    "unit": "void",
    "int": "int",
    "float": "float",
    "bool": "bool",
    "string": "string",
    "char": "char",
}
IR_WIDTH_TYPES = {
    "i8": "ExactInt<i8>",
    "i16": "ExactInt<i16>",
    "i32": "ExactInt<i32>",
    "i64": "ExactInt<i64>",
    "u8": "ExactInt<u8>",
    "u16": "ExactInt<u16>",
    "u32": "ExactInt<u32>",
    "u64": "ExactInt<u64>",
    "f32": "ExactFloat<f32>",
    "f64": "ExactFloat<f64>",
}
ALLOWED_SCOPE_KEYS = {"host", "method", "table", "path_prefix"}
# Scope predicates consumed by the governance layer (R-3214).  Keep this table
# small: an entry belongs here only when a scope extractor exists beside the
# host call.
SCOPE_KEY_OVERRIDES = {
    # HTTP client requests are scoped by destination host and method.
    "std.api.client.request": ["host", "method"],
    # Schema migrations write the tracked migration table.
    "std.api.db.migrate.apply_sqlite": ["table"],
}


def read_expression(text: str, index: int) -> str:
    """Return the Rust expression starting at ``index`` up to its comma."""
    depth = 0
    cursor = index
    while cursor < len(text):
        char = text[cursor]
        if char in "{(<[":
            depth += 1
        elif char in "})>]":
            depth -= 1
        elif char == "," and depth == 0:
            break
        cursor += 1
    return text[index:cursor]


def struct_ir_name(name: str) -> str:
    """Render a midend struct name with the catalog IR type grammar."""
    for prefix in ("List", "Set", "Iterator"):
        if name.startswith(prefix + "_"):
            return f"{prefix}<{name[len(prefix) + 1:]}>"
    if name.startswith("Map_"):
        key, value = name[len("Map_") :].split("_", 1)
        return f"Map<{key},{value}>"
    return name


def ir_type_text(expression: str) -> str:
    """Render a midend `IRType` expression with the catalog IR type grammar."""
    expr = re.sub(r"\s+", " ", expression).strip()
    scalars = {
        "IRType::Int": "int",
        "IRType::Float": "float",
        "IRType::Bool": "bool",
        "IRType::String": "string",
        "IRType::Void": "void",
        "IRType::Unknown": "unknown",
        "IRType::Range": "range",
        "IRType::Char": "char",
    }
    if expr in scalars:
        return scalars[expr]
    if expr == "builtin_error_ir_type()":
        return "Error"
    match = re.fullmatch(r"builtin_result_ir_type\((.+)\)", expr)
    if match:
        return f"Result<{ir_type_text(match.group(1))}>"
    match = re.fullmatch(
        r'IRType::Struct \{ name: "([A-Za-z0-9_]+)"\.to_string\(\), fields: Vec::new\(\), \}',
        expr,
    )
    if match:
        return struct_ir_name(match.group(1))
    match = re.fullmatch(
        r'IRType::Enum \{ name: "([A-Za-z0-9_]+)"\.to_string\(\), variants: .*\}',
        expr,
    )
    if match:
        name = match.group(1)
        if name.startswith("Option_"):
            return f"Option<{struct_ir_name(name[len('Option_') :])}>"
        if name.startswith("Result_"):
            return f"Result<{struct_ir_name(name[len('Result_') :])}>"
        return struct_ir_name(name)
    match = re.fullmatch(
        r"IRType::Tensor \{ dtype: Box::new\((IRType::[A-Za-z]+)\), "
        r"rank: (Some\(\d+\)|None), dims: None, layout: None, device: None, \}",
        expr,
    )
    if match:
        dtype = ir_type_text(match.group(1))
        rank = match.group(2)
        if rank == "None":
            return f"Tensor<{dtype}>"
        return f"Tensor<{dtype},rank={re.search(r'[0-9]+', rank).group(0)}>"
    match = re.fullmatch(r"IRType::ExactFloat \{ width: IRFloatWidth::(F32|F64), \}", expr)
    if match:
        return f"ExactFloat<{match.group(1).lower()}>"
    match = re.fullmatch(r"IRType::ExactInt \{ signed, width \}", expr)
    if match:
        # Computed from the function name by the numeric table; kept explicit so
        # an unexpected form cannot enter the catalog silently.
        return "ExactInt<var>"
    raise RuntimeError(f"unrecognized IR return expression: {expr}")


def parse_descriptor(body: str) -> dict[str, object]:
    """Return the host descriptor fields for one lowering match arm."""
    runtime_name = re.search(r'runtime_name:\s*"([^"]+)"', body)
    match = re.search(r"return_type:\s*", body)
    if match:
        ir_return = ir_type_text(read_expression(body, match.end()))
        returns_value = re.search(r"returns_value:\s*(true|false)", body[match.end() :])
        if returns_value is None:
            raise RuntimeError(f"lowering arm without returns_value: {body[:160]!r}")
        return {
            "ir_return": ir_return,
            "returns_value": returns_value.group(1) == "true",
            "runtime_name": runtime_name.group(1) if runtime_name else "",
        }
    for helper, descriptor in HOST_DESCRIPTOR_HELPERS.items():
        if re.search(r"\b" + helper + r"\(", body):
            helper_name = re.search(r'\b' + helper + r'\(\s*"([^"]+)"', body)
            return {
                "ir_return": descriptor[0],
                "returns_value": descriptor[1],
                "runtime_name": helper_name.group(1) if helper_name else "",
            }
    raise RuntimeError(f"lowering arm without a host descriptor: {body[:160]!r}")


def lowering_descriptors(root: Path) -> dict[str, dict[str, object]]:
    """Inventory the hand-written midend host descriptor tables."""
    descriptors: dict[str, dict[str, object]] = {}
    for scope, relative in LOWERING_TABLES:
        text = (root / relative).read_text(encoding="utf-8")
        arms = list(LOWERING_ARM_RE.finditer(text))
        for index, arm in enumerate(arms):
            end = arms[index + 1].start() if index + 1 < len(arms) else len(text)
            module, function = arm.group(1), arm.group(2)
            prefix = "std.api." if scope == "api" else "std."
            descriptors[prefix + module + "." + function] = parse_descriptor(text[arm.end() : end])
    return descriptors


def host_call_symbols(root: Path) -> dict[str, tuple[str, str]]:
    """Map `std.api.*` paths to (rust_symbol, cfg_feature) from host_calls.rs."""
    text = (root / HOST_CALLS_SOURCE).read_text(encoding="utf-8")
    symbols: dict[str, tuple[str, str]] = {}
    for feature, name, function in HOST_CALL_RE.findall(text):
        if not name.startswith(HOST_CALLS_PREFIX):
            continue
        symbols["std.api." + name[len(HOST_CALLS_PREFIX) :]] = (function, feature)
    return symbols


def signature_parts(signature: str) -> tuple[list[dict[str, str]], str]:
    """Split `fn(a, b) -> c` into anonymous parameters and the return type."""
    open_paren = signature.find("(")
    if open_paren < 0:
        raise RuntimeError(f"signature without parameters: {signature!r}")
    depth = 0
    close_paren = -1
    for index in range(open_paren, len(signature)):
        if signature[index] == "(":
            depth += 1
        elif signature[index] == ")":
            depth -= 1
            if depth == 0:
                close_paren = index
                break
    if close_paren < 0:
        raise RuntimeError(f"signature without closing parenthesis: {signature!r}")
    raw_args = signature[open_paren + 1 : close_paren].strip()
    types = [arg.strip() for arg in raw_args.split(",")] if raw_args else []
    params = [
        {"name": f"arg{index}", "ty": ty}
        for index, ty in enumerate(types)
        if ty
    ]
    tail = signature[close_paren + 1 :].strip()
    returns = tail[2:].strip() if tail.startswith("->") else "unit"
    return params, returns


def signature_ir_return(returns: str) -> str:
    """Map a semantic return type onto the catalog IR type grammar."""
    value = re.sub(r"\s*,\s*", ",", returns.strip())
    if value in IR_SCALAR_TYPES:
        return IR_SCALAR_TYPES[value]
    if value in IR_WIDTH_TYPES:
        return IR_WIDTH_TYPES[value]
    if value in {"int_tensor", "float_tensor", "bool_tensor", "string_tensor"}:
        return f"Tensor<{value[: -len('_tensor')]}>"
    if _is_ir_type_expression(value):
        return value
    raise RuntimeError(f"cannot map semantic return type {returns!r} onto the IR grammar")


def _is_ir_type_expression(value: str) -> bool:
    """Accept `Name` or `Name<arg[,arg]*>` nested to any depth.

    Async host returns such as `Task<Result<Tensor<float, rank=1>, Error>>`
    need arbitrary nesting; the previous single-level regex rejected them.
    """
    if not value or not re.match(r"[A-Za-z_]", value):
        return False
    if not re.fullmatch(r"[A-Za-z0-9_\.<>,= ]+", value):
        return False
    depth = 0
    for char in value:
        if char == "<":
            depth += 1
        elif char == ">":
            depth -= 1
            if depth < 0:
                return False
    return depth == 0



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
    lowering = lowering_descriptors(ROOT)
    host_calls = host_call_symbols(ROOT)
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
        effects = old.get("effects", ["host"] if kind == "function" else [])
        binding = binding_for(path, kind)
        params: list[dict[str, str]] = []
        returns = ""
        ir_return = ""
        returns_value = False
        rust_symbol = ""
        cfg_feature = ""
        scope_keys: list[str] = []
        if kind == "function":
            params, returns = signature_parts(signature)
            descriptor = lowering.get(path)
            if descriptor is not None:
                ir_return = str(descriptor["ir_return"])
                returns_value = bool(descriptor["returns_value"])
            else:
                # Compiler-emitted host calls (JSON derive helpers, version
                # probes, tensor literals) have no table arm; their semantic
                # signature is the only return-type source.
                ir_return = signature_ir_return(returns)
                returns_value = ir_return != "void"
            if path.startswith("std.api."):
                host = host_calls.get(path)
                if host is not None:
                    rust_symbol, cfg_feature = host
                elif path in audit.HOST_CALL_ALIASES:
                    # The alias owns no HostCallSpec; its host binding is the
                    # sibling runtime name recorded by the lowering table.
                    runtime_name = str(descriptor["runtime_name"]) if descriptor else ""
                    if runtime_name:
                        binding = runtime_name
                else:
                    raise RuntimeError(
                        f"std.api function without a HostCallSpec rust_symbol: {path}"
                    )
            scope_keys = list(SCOPE_KEY_OVERRIDES.get(path, []))
            unknown = [key for key in scope_keys if key not in ALLOWED_SCOPE_KEYS]
            if unknown:
                raise RuntimeError(f"unknown scope keys for {path}: {unknown}")
        entry = {
            "path": path,
            "kind": kind,
            "namespace": namespace_for(path, kind),
            # Signatures are compiler-owned and must never be preserved from
            # a stale catalog entry. ABI/docs metadata may be migrated, but a
            # changed semantic contract must be visible in the generated file.
            "signature": signature,
            "params": params,
            "returns": returns,
            "ir_return": ir_return,
            "returns_value": returns_value,
            "rust_symbol": rust_symbol,
            "cfg_feature": cfg_feature,
            "abi": old.get(
                "abi",
                "semantic descriptor" if kind != "function" else "host(ctx: SpectraHostCallContext) -> i32",
            ),
            "effects": effects,
            "sink": "mutation" in effects,
            "scope_keys": scope_keys,
            "error_model": old.get(
                "error_model",
                "none" if kind != "function" else (
                    "legacy compatibility adapter" if path.startswith("std.compat.") else "host status + typed return"
                ),
            ),
            "binding": binding,
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
        for key in ("path", "kind", "namespace", "signature"):
            lines.append(f"{key} = {toml_string(str(entry[key]))}")
        if entry["kind"] == "function":
            params = entry["params"]
            rendered = ", ".join(
                "{ name = " + toml_string(str(param["name"])) + ", ty = " + toml_string(str(param["ty"])) + " }"
                for param in params
            )
            lines.append(f"params = [{rendered}]")
            lines.append(f"returns = {toml_string(str(entry['returns']))}")
            lines.append(f"ir_return = {toml_string(str(entry['ir_return']))}")
            lines.append(f"returns_value = {'true' if entry['returns_value'] else 'false'}")
            if entry["rust_symbol"]:
                lines.append(f"rust_symbol = {toml_string(str(entry['rust_symbol']))}")
            if entry["cfg_feature"]:
                lines.append(f"cfg_feature = {toml_string(str(entry['cfg_feature']))}")
        lines.append(f"abi = {toml_string(str(entry['abi']))}")
        effects = entry["effects"]
        lines.append("effects = [" + ", ".join(toml_string(str(value)) for value in effects) + "]")
        if entry["sink"]:
            lines.append("sink = true")
        if entry["scope_keys"]:
            lines.append(
                "scope_keys = ["
                + ", ".join(toml_string(str(key)) for key in entry["scope_keys"])
                + "]"
            )
        for key in ("error_model", "binding", "maturity", "owner", "docs", "fixture"):
            lines.append(f"{key} = {toml_string(str(entry[key]))}")
    current_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"generated {len(entries)} catalog entries at {current_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
