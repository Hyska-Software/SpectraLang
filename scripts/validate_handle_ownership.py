"""Validate the generational handle/lifecycle contract used by runtime domains."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path


SCHEMA = "spectralang.stability.handle_ownership.v1"


def command(root: Path, args: list[str], timeout: int) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=root,
        text=True,
        capture_output=True,
        timeout=timeout,
        check=False,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/debug/spectralang.exe")
    parser.add_argument(
        "--fixture", default="tests/validation/stability_handle_lifecycle.spectra"
    )
    parser.add_argument("--report", default="target/stability/handle-ownership.json")
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    report_path = (root / args.report).resolve()
    report_path.parent.mkdir(parents=True, exist_ok=True)
    handle_source = root / "runtime" / "src" / "handles" / "mod.rs"
    registry_source = root / "runtime" / "src" / "stdlib" / "mod.rs"
    api_source = root / "packages" / "spectra-api" / "src"

    result: dict[str, object] = {
        "schema": SCHEMA,
        "status": "failed",
        "fixture": str((root / args.fixture).resolve()),
        "binary": str((root / args.binary).resolve()),
    }
    failures: list[str] = []

    if not handle_source.is_file():
        failures.append("runtime/src/handles/mod.rs is missing")
    else:
        source = handle_source.read_text(encoding="utf-8")
        kind_names = re.findall(r"^    ([A-Za-z][A-Za-z0-9_]*) = \d+,?$", source, re.MULTILINE)
        required_tokens = [
            "pub struct HandleId",
            "pub struct HandleTable",
            "generation",
            "pub fn remove",
            "HandleError::Stale",
        ]
        missing = [token for token in required_tokens if token not in source]
        result["handle_kind_count"] = len(kind_names)
        result["required_tokens"] = {token: token not in missing for token in required_tokens}
        if missing:
            failures.append("handle table source is missing: " + ", ".join(missing))

    registry_text = (
        "\n".join(
            path.read_text(encoding="utf-8")
            for path in sorted((root / "runtime" / "src" / "stdlib").rglob("*.rs"))
        )
        if (root / "runtime" / "src" / "stdlib").is_dir()
        else ""
    )
    api_text = "\n".join(path.read_text(encoding="utf-8") for path in api_source.glob("*.rs")) if api_source.is_dir() else ""
    result["runtime_registry_uses_generational_tables"] = "HandleTable::new" in registry_text
    result["api_registry_uses_generational_tables"] = "ApiHandleTable" in api_text
    if "HandleTable::new" not in registry_text:
        failures.append("runtime stdlib registry no longer references HandleTable")
    if "ApiHandleTable" not in api_text:
        failures.append("API handle adapters are missing")

    fixture = command(root, [str((root / args.binary).resolve()), "run", str((root / args.fixture).resolve())], 120)
    result["fixture_run"] = {
        "exit_code": fixture.returncode,
        "stdout_tail": fixture.stdout[-2000:],
        "stderr_tail": fixture.stderr[-2000:],
    }
    if fixture.returncode != 0:
        failures.append("stale-handle lifecycle fixture failed")

    runtime_tests = command(root, ["cargo", "test", "-p", "spectra-runtime", "--lib"], 240)
    result["runtime_tests"] = {
        "exit_code": runtime_tests.returncode,
        "stdout_tail": runtime_tests.stdout[-2000:],
        "stderr_tail": runtime_tests.stderr[-2000:],
    }
    if runtime_tests.returncode != 0:
        failures.append("runtime handle/lifecycle test suite failed")

    result["failures"] = failures
    result["status"] = "passed" if not failures else "failed"
    report_path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"schema": SCHEMA, "status": result["status"], "failures": failures}, indent=2))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
