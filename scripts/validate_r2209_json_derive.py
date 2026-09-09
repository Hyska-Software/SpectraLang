from __future__ import annotations

import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CARGO = r"C:\Users\estev\.cargo\bin\cargo.exe"
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"


def read(path: str) -> str:
    source = ROOT / path
    if source.suffix == ".rs":
        return "\n".join(
            sibling.read_text(encoding="utf-8")
            for sibling in sorted(source.parent.rglob("*.rs"))
        )
    return source.read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2209 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_command(args: list[str], expect_success: bool = True) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if expect_success and completed.returncode != 0:
        fail(f"command {' '.join(args)} failed:\n{completed.stdout}")
    if not expect_success and completed.returncode == 0:
        fail(f"command {' '.join(args)} unexpectedly succeeded:\n{completed.stdout}")
    return completed.stdout


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


def validate_surface() -> None:
    ast = read("compiler/src/ast/mod.rs")
    parser = read("compiler/src/parser/item.rs")
    semantic = read("compiler/src/semantic/mod.rs")
    midend = read("midend/src/lowering.rs")

    for term in [
        "pub struct Attribute",
        "pub enum AttributeArgument",
        "pub attributes: Vec<Attribute>",
    ]:
        require(term in ast, f"AST missing {term}")

    for term in [
        "parse_attribute_arguments",
        "AttributeArgument::KeyValue",
        "parse_struct(visibility, attributes)",
        "parse_enum(visibility, attributes)",
        "field_attributes",
        "variant_attributes",
    ]:
        require(term in parser, f"parser missing {term}")

    for term in [
        "JsonDeriveSet",
        "JsonDerivedStructInfo",
        "validate_json_struct_derives",
        "validate_json_enum_derives",
        "register_json_derived_methods",
        "validate_derived_from_json_literal",
        "std.api.json",
        "EJSON003",
        "EJSON004",
        "json_error_field",
    ]:
        require(term in semantic, f"semantic implementation missing {term}")

    for term in [
        'method_name == "to_json"',
        'variant_name == "from_json"',
        'variant_name == "json_error_field"',
        "lower_default_struct_value",
    ]:
        require(term in midend, f"midend lowering missing {term}")


def validate_fixtures() -> None:
    for path in [
        "tests/validation/133_json_derive_surface.spectra",
        "tests/errors/json_derive_missing_field.spectra",
        "tests/errors/json_derive_wrong_type.spectra",
        "tests/errors/json_derive_duplicate_rename.spectra",
        "tests/errors/json_derive_invalid_attribute.spectra",
    ]:
        require((ROOT / path).is_file(), f"missing fixture {path}")

    valid = read("tests/validation/133_json_derive_surface.spectra")
    for term in [
        "#[derive(Serialize, Deserialize)]",
        "#[json(rename = \"user_id\")]",
        "#[json(optional)]",
        "profile.to_json()",
        "Profile::from_json",
        "Profile::json_error_field",
    ]:
        require(term in valid, f"valid fixture missing {term}")


def validate_docs_and_planning() -> None:
    docs = read("docs/api/std-api-json-derive.md")
    for term in [
        "#[derive(Serialize, Deserialize)]",
        "std.api.json.quote_string",
        "std.api.json.encode_number",
        "std.api.json.decode_field",
        "std.api.json.typed_error_field",
        "rename",
        "optional",
        "EJSON003",
        "EJSON004",
    ]:
        require(term in docs, f"derive docs missing {term}")

    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2209 = items.get("R-2209")
    require(r2209 is not None, "R-2209 missing from roadmap")
    require(r2209.get("status") == "complete", "R-2209 must be marked complete")
    require(r2209.get("dependencies") == ["R-2208"], "R-2209 must depend on R-2208")
    acceptance = "\n".join(r2209.get("acceptance", []))
    for term in [
        "derive macro generates code that uses `std.api.json.*`",
        "optional fields and explicit renaming are supported",
        "typed error that points to the failing field",
        "happy path, missing field, wrong type, duplicate rename, and invalid json attribute",
        "scripts/validate_r2209_json_derive.py",
    ]:
        require(term in acceptance, f"R-2209 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2209 JSON Derive: Serialize and Deserialize", 1)[1].split("## R-2210", 1)[0]
    for term in [
        "Status: `complete`",
        "compiler/src/parser/item.rs",
        "compiler/src/semantic/mod.rs",
        "midend/src/lowering.rs",
        "docs/api/std-api-json-derive.md",
        "validate_r2209_json_derive.py",
    ]:
        require(term in block, f"backlog R-2209 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2209` JSON derive: `Serialize` and `Deserialize` (complete;" in plan,
        "implementation plan must mark R-2209 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2209_json_derive.py" in runner, "run_tests.ps1 must run R-2209 validator")
    require('Teste = "validate_r2209_json_derive"' in runner, "run_tests.ps1 must record R-2209")


def run_regressions() -> None:
    run_command([CARGO, "test", "-q", "-p", "spectra-compiler", "--offline"])
    run_command([CARGO, "test", "-q", "-p", "spectra-midend", "--offline"])
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(SPECTRALANG), "compile", "tests/validation/133_json_derive_surface.spectra"])
    run_command([str(SPECTRALANG), "run", "tests/validation/133_json_derive_surface.spectra"])
    run_command([str(SPECTRALANG), "run", "tests/validation/357_json_derive_roundtrip.spectra"])
    malformed = run_command(
        [str(SPECTRALANG), "run", "tests/errors/json_malformed_rejects.spectra"],
        expect_success=False,
    )
    require(
        "decode error at 'user_id'" in malformed,
        "malformed JSON must name the violating field path",
    )
    expected_errors = {
        "tests/errors/json_derive_missing_field.spectra": "missing required field 'name'",
        "tests/errors/json_derive_wrong_type.spectra": "field 'user_id' has wrong type",
        "tests/errors/json_derive_duplicate_rename.spectra": "Duplicate JSON field name 'id'",
        "tests/errors/json_derive_invalid_attribute.spectra": "Unsupported json option 'skip'",
    }
    for fixture, expected in expected_errors.items():
        output = run_command([str(SPECTRALANG), "check", fixture], expect_success=False)
        require(expected in output, f"{fixture} did not report expected diagnostic {expected}")


def main() -> None:
    validate_surface()
    validate_fixtures()
    run_regressions()
    validate_docs_and_planning()
    validate_runner()
    print("validated R-2209 JSON derive Serialize/Deserialize")


if __name__ == "__main__":
    main()
