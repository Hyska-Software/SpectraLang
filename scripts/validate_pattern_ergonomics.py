#!/usr/bin/env python3
"""Validate R-203 destructuring and pattern ergonomics coverage."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


REQUIRED_SOURCE_MARKERS = {
    # These stages are intentionally decomposed into multiple Rust modules.
    # Validate the stage boundary rather than pinning the contract to the old
    # pre-decomposition monolith files.
    "compiler/src/parser": [
        "Pattern::Tuple",
        "Pattern::Struct",
        "Pattern::EnumVariant",
        "Pattern::Or",
    ],
    "compiler/src/semantic": [
        "Pattern::Tuple",
        "Pattern::Struct",
        "Pattern::EnumVariant",
        "Pattern::Or",
        "check_match_exhaustiveness",
    ],
    "midend/src": [
        "Pattern::Tuple",
        "Pattern::Struct",
        "Pattern::EnumVariant",
        "Pattern::Or",
    ],
}

POSITIVE_TESTS = [
    "tests/validation/31_tuple_variant_destructuring.spectra",
    "tests/validation/60_pattern_control_surface.spectra",
    "tests/validation/63_destructuring_and_or_patterns.spectra",
]

NEGATIVE_TESTS = [
    "tests/errors/non_exhaustive_enum_match.spectra",
]


def run(binary: Path, args: list[str], root: Path) -> tuple[int, str]:
    proc = subprocess.run(
        [str(binary), *args],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        timeout=20,
    )
    return proc.returncode, proc.stdout


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--binary", default="target/debug/spectralang.exe")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    binary = (root / args.binary).resolve()
    errors: list[str] = []

    if not binary.is_file():
        errors.append(f"missing CLI binary: {binary}")

    for rel, markers in REQUIRED_SOURCE_MARKERS.items():
        path = root / rel
        if not path.is_dir():
            errors.append(f"missing source directory: {rel}")
            continue
        source_files = sorted(path.rglob("*.rs"))
        text = "\n".join(file.read_text(encoding="utf-8") for file in source_files)
        for marker in markers:
            if marker not in text:
                errors.append(f"{rel}/** missing marker: {marker}")

    for rel in POSITIVE_TESTS:
        path = root / rel
        if not path.is_file():
            errors.append(f"missing positive pattern test: {rel}")
            continue
        code, output = run(binary, ["compile", str(path)], root)
        if code != 0:
            errors.append(f"positive pattern test failed: {rel}: {output[-400:]}")

    for rel in NEGATIVE_TESTS:
        path = root / rel
        if not path.is_file():
            errors.append(f"missing negative pattern test: {rel}")
            continue
        code, output = run(binary, ["check", str(path)], root)
        if code == 0:
            errors.append(f"negative pattern test unexpectedly passed: {rel}")
        if "not exhaustive" not in output and "non-exhaustive" not in output:
            errors.append(f"negative pattern test did not report exhaustiveness: {rel}")

    if errors:
        for error in errors:
            print(f"ERROR: {error}")
        return 1

    print("validated R-203 pattern ergonomics")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
