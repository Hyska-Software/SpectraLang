# R-3215 — capability vocabulary validated by the compiler.
#
# Validates the compile-time capability vocabulary: a literal `agent_start`
# spec has its `allow` grants resolved against the contract catalog, unknown
# grants fail with `E3201` and a did-you-mean, scope predicates on host calls
# without an extractor fail with `E3202`, valid grants compile, and the
# generated capability reference matches the catalog.
from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
CARGO = shutil.which("cargo") or "cargo"
CATALOG = ROOT / "packages" / "spectra-contract" / "catalog" / "stdlib.toml"
DOC = "docs/agent-platform.md"
POSITIVE = "tests/validation/374_agent_capability_vocabulary.spectra"
REJECTIONS = {
    "tests/errors/capability_unknown.spectra": "E3201",
    "tests/errors/capability_unsupported_scope.spectra": "E3202",
}


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3215 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run(args: list[str]) -> tuple[int, str]:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    return completed.returncode, completed.stdout


def run_command(args: list[str]) -> str:
    exit_code, output = run(args)
    if exit_code != 0:
        fail(f"command {' '.join(args)} failed with {exit_code}:\n{output}")
    return output


def parse_json_line(output: str) -> dict:
    """Parse the first output line as JSON (stderr is merged for diagnostics)."""
    return json.loads(output.splitlines()[0])


def diagnostics_of(report: dict) -> list[dict]:
    return [d for file in report.get("files", []) for d in file.get("diagnostics", [])]


def ensure_binary() -> None:
    if SPECTRALANG.exists():
        return
    run_command([CARGO, "build", "-p", "spectra-cli", "--offline"])


def validate_implementation() -> None:
    agent = read("compiler/src/semantic/semantic_agent.rs")
    for term in [
        "validate_agent_start_capabilities",
        "validate_capability",
        "capability_vocabulary",
        "spectra_contract::catalog",
        '"E3201"',
        '"E3202"',
    ]:
        require(term in agent, f"semantic_agent.rs missing {term}")

    calls = read("compiler/src/semantic/semantic_expression_calls.rs")
    require(
        "validate_agent_start_capabilities" in calls,
        "the call analysis must validate a literal agent_start spec",
    )

    docs = read("docs/diagnostics/error-code-reference.md")
    for code in ["`E3201`", "`E3202`"]:
        require(code in docs, f"error-code-reference.md must document {code}")


def validate_positive_fixture() -> None:
    exit_code, output = run([str(SPECTRALANG), "check", "--json", POSITIVE])
    require(exit_code == 0, f"check on the positive fixture exited {exit_code}:\n{output}")
    report = parse_json_line(output)
    diagnostics = diagnostics_of(report)
    require(not diagnostics, f"valid capability grants must be diagnostic-free: {diagnostics}")


def validate_rejections() -> None:
    for fixture, code in REJECTIONS.items():
        exit_code, output = run([str(SPECTRALANG), "check", "--json", fixture])
        require(exit_code != 0, f"{fixture} must fail compilation:\n{output}")
        report = parse_json_line(output)
        diagnostics = diagnostics_of(report)
        matching = [d for d in diagnostics if d.get("code") == code]
        require(
            len(matching) == 1,
            f"{fixture} must report exactly one {code}, got {[d.get('code') for d in diagnostics]}",
        )
        hint = matching[0].get("hint") or ""
        require(hint, f"{fixture} must carry a hint for {code}")
        if code == "E3201":
            require(
                "Did you mean" in hint and "spectra.std.fs.fs_read" in hint,
                f"{fixture} must suggest the nearest catalog name: {hint}",
            )
        else:
            require(
                "supports" in hint,
                f"{fixture} must name the supported scope keys: {hint}",
            )


def validate_reference() -> None:
    exit_code, output = run([sys.executable, "scripts/generate_capability_reference.py", "--check"])
    require(exit_code == 0, f"generated capability reference drifted from the catalog:\n{output}")

    catalog = tomllib.loads(CATALOG.read_text(encoding="utf-8"))
    calls = [
        entry
        for entry in catalog["entry"]
        if entry.get("abi", "").startswith("host(")
    ]
    namespaces = sorted(
        {entry["binding"].rpartition(".")[0] for entry in calls}
    )
    scoped = sorted(
        (entry["binding"], entry.get("scope_keys", []))
        for entry in calls
        if entry.get("scope_keys")
    )

    document = read(DOC)
    require(
        f"{len(namespaces)} namespace grants cover {len(calls)} host calls." in document,
        "the generated reference must count the catalog host calls",
    )
    for namespace in namespaces:
        require(
            f"| `{namespace}` |" in document,
            f"the generated reference is missing namespace {namespace}",
        )
    for binding, keys in scoped:
        row = f"| `{binding}` |"
        require(row in document, f"the generated reference is missing scoped host call {binding}")
        line = next(line for line in document.splitlines() if line.startswith(row))
        for key in keys:
            require(f"`{key}`" in line, f"{binding} must document scope key {key}")

    require(
        "agent-platform-plan.md" in document and "adr/0016-agent-capability-enforcement.md" in document,
        "docs/agent-platform.md must point at the plan and the security model",
    )


def main() -> None:
    ensure_binary()
    validate_implementation()
    validate_positive_fixture()
    validate_rejections()
    validate_reference()
    print("validated R-3215 capability vocabulary")


if __name__ == "__main__":
    main()
