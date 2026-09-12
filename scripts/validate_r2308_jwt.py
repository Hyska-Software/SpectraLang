from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 557


def cargo_command() -> str:
    cargo = "cargo.exe" if sys.platform.startswith("win") else "cargo"
    home = Path(os.environ.get("USERPROFILE") or os.environ.get("HOME") or "")
    candidate = home / ".cargo" / "bin" / cargo
    return str(candidate) if candidate.exists() else "cargo"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2308 validation failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_command(args: list[str]) -> None:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if completed.returncode != 0:
        fail(f"command {' '.join(args)} failed:\n{completed.stdout}")


def validate_implementation() -> None:
    jwt = read("packages/spectra-api/src/jwt.rs")
    for term in [
        "pub enum JwtAlgorithm",
        "Hs256",
        "Rs256",
        "Es256",
        "MIN_HS256_KEY_BYTES",
        "hmac::verify",
        "RSA_PKCS1_2048_8192_SHA256",
        "ECDSA_P256_SHA256_FIXED",
        "validate_claims",
        '"exp"',
        '"nbf"',
        '"iss"',
        '"aud"',
        '"sub"',
        '"jti"',
        "pub extern \"C\" fn jwt_sign",
        "pub extern \"C\" fn jwt_verify",
        "jwt_hs256_validates_claims_and_rejects_tampering",
        "jwt_rs256_signs_and_verifies_with_der_keys",
        "jwt_es256_signs_and_verifies_with_generated_p256_keys",
    ]:
        require(term in jwt, f"jwt.rs missing {term}")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("packages/spectra-api/src/host_calls.rs")
    api_tests = read("packages/spectra-api/src/api_tests.rs")
    for name in ["spectra.api.jwt.sign", "spectra.api.jwt.verify"]:
        require(name in host_calls, f"{name} missing from package host registry")
        require(name in runtime_api, f"{name} missing from runtime contract")
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})"
        in read("packages/spectra-api/src/api_tests.rs"),
        "package host-call count is not synchronized",
    )
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})"
        in api_tests,
        "package host-call count is not synchronized",
    )


def validate_surface() -> None:
    sources = [
        read("compiler/src/semantic/builtin_api_core.rs"),
        read("compiler/src/semantic/builtin_contract.rs"),
        read("midend/src/lowering_std_api.rs"),
        read("compiler/tests/snapshots/std_api_public_function_table.snap"),
    ]
    for term in ["std.api.jwt", "std.api.jwt.sign", "std.api.jwt.verify"]:
        require(any(term in source for source in sources), f"public surface missing {term}")


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/335_api_jwt.spectra")
    for term in [
        'sign("HS256"',
        "verify(",
        "wrong-issuer",
        "1700000100",
        "1700000101",
        "token + \"x\"",
    ]:
        require(term in fixture, f"JWT fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.jwt"',
        'path = "std.api.jwt.sign"',
        'path = "std.api.jwt.verify"',
        'fixture = "tests/validation/335_api_jwt.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-jwt.md")
    for term in [
        "HS256",
        "RS256",
        "ES256",
        "exp",
        "nbf",
        "iss",
        "aud",
        "335_api_jwt.spectra",
    ]:
        require(term in docs, f"JWT docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2308")
    require(item is not None, "R-2308 missing from roadmap")
    require(item.get("status") == "complete", "R-2308 is not complete")
    require(item.get("owner") == "web", "R-2308 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "each documented algorithm",
        "expired",
        "not-yet-valid",
        "wrong-issuer",
        "tampered payload",
        "335_api_jwt.spectra",
        "validate_r2308_jwt.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2308 JWT (HS256, RS256, ES256)", 1)[1].split(
        "## R-2309", 1
    )[0]
    for term in [
        "Status: `complete`",
        "335_api_jwt.spectra",
        "validate_r2308_jwt.py",
    ]:
        require(term in block, f"backlog R-2308 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2308_jwt.py" in runner, "runner does not invoke R-2308")
    require(
        'Teste = "validate_r2308_jwt"' in runner,
        "runner does not record R-2308",
    )


def validate_commands() -> None:
    cargo = cargo_command()
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    run_command([cargo, "test", "-q", "-p", "spectra-api", "--lib", "jwt", "--offline"])
    run_command(
        [
            cargo,
            "test",
            "-q",
            "-p",
            "spectra-compiler",
            "--test",
            "snapshot_tests",
            "--offline",
        ]
    )
    run_command([cargo, "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "check", "tests/validation/335_api_jwt.spectra"])
    run_command([str(binary), "run", "tests/validation/335_api_jwt.spectra"])


def main() -> None:
    validate_implementation()
    validate_surface()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2308 JWT")


if __name__ == "__main__":
    main()
