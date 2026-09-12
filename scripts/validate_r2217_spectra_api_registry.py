#!/usr/bin/env python3
"""Validate R-2217 spectra.api local registry publishing."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print(f"R-2217 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def read(path: str) -> str:
    source = ROOT / path
    if source.suffix == ".rs":
        return "\n".join(
            sibling.read_text(encoding="utf-8")
            for sibling in sorted(source.parent.rglob("*.rs"))
        )
    return source.read_text(encoding="utf-8")


def parse_toml(path: Path):
    with path.open("rb") as fh:
        return tomllib.load(fh)


def run_command(args: list[str], cwd: Path = ROOT, timeout: int = 60) -> str:
    completed = subprocess.run(
        args,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
        check=False,
    )
    if completed.returncode != 0:
        fail(f"command {' '.join(args)} failed:\n{completed.stdout}")
    return completed.stdout


def cargo_cmd() -> str:
    configured = os.environ.get("CARGO")
    if configured:
        return configured
    found = shutil.which("cargo")
    if found:
        return found
    windows_default = Path.home() / ".cargo" / "bin" / "cargo.exe"
    if windows_default.exists():
        return str(windows_default)
    return "cargo"


def validate_static_files() -> None:
    manifest = parse_toml(ROOT / "packages/spectra-api/spectra.toml")
    require(manifest["project"]["name"] == "spectra.api", "package name must be spectra.api")
    require(manifest["project"]["version"] == "0.1.0", "package version must be deterministic")
    require(
        manifest["project"]["entry"] == "src/bindings/mod.spectra",
        "package entry must point at bindings/mod.spectra",
    )
    require(
        manifest["project"]["src_dirs"] == ["src/bindings"],
        "package source roots must be limited to public Spectra bindings",
    )
    release = manifest.get("release", {})
    require(release.get("channel") == "beta", "manifest must declare beta release channel")
    require(
        release.get("compatibility") == "spectralang-0.1",
        "manifest must pin compatible Spectra runtime level",
    )

    parser = read("compiler/src/parser/module.rs")
    require(
        "Expected identifier after '.' in module name" in parser and "name.push('.')" in parser,
        'parser must accept dotted package binding module names',
    )

    package_rs = read("tools/spectra-cli/src/package.rs")
    for term in [
        "source_path",
        "registry_package_dir",
        "name.replace('-', \".\")",
        "doc[\"dependencies\"][dependency_name]",
        "canonical_name: metadata.name",
    ]:
        require(term in package_rs, f"package CLI missing {term}")

    binding_root = read("packages/spectra-api/src/bindings/mod.spectra")
    require('module spectra_api' in binding_root, "binding root must be parseable")
    require('public func main() returns int' in binding_root, "package run must have an entrypoint")


def validate_registry_flow(binary: Path) -> None:
    work = ROOT / "target" / "r2217-spectra-api-registry"
    registry = work / "registry"
    consumer = work / "consumer"
    if work.exists():
        shutil.rmtree(work)
    consumer_src = consumer / "src"
    consumer_src.mkdir(parents=True)
    consumer_src.joinpath("main.spectra").write_text(
        'module consumer\n\npublic func main() returns int {\n    return 0\n}\n',
        encoding="utf-8",
    )
    consumer.joinpath("spectra.toml").write_text(
        "\n".join(
            [
                "[project]",
                'name = "spectra_api_consumer"',
                'version = "0.1.0"',
                'entry = "src/main.spectra"',
                'src_dirs = ["src"]',
                "",
                "[release]",
                'channel = "nightly"',
                'compatibility = "spectralang-0.1"',
                "",
                "[dependencies]",
                "",
            ]
        ),
        encoding="utf-8",
    )

    run_command([str(binary), "package", "publish", "--root", "packages/spectra-api", "--registry", str(registry)])
    metadata_path = registry / "spectra.api" / "0.1.0" / "package.toml"
    require(metadata_path.is_file(), "registry metadata for spectra.api 0.1.0 missing")
    metadata = parse_toml(metadata_path)
    require(metadata.get("name") == "spectra.api", "registry metadata has wrong package name")
    require(metadata.get("version") == "0.1.0", "registry metadata has wrong version")
    require(metadata.get("channel") == "beta", "registry metadata missing release channel")
    require(
        metadata.get("compatibility") == "spectralang-0.1",
        "registry metadata missing compatibility",
    )
    require(isinstance(metadata.get("checksum"), str) and metadata["checksum"], "checksum missing")
    require(
        "packages/spectra-api" in metadata.get("source_path", "").replace("\\", "/"),
        "source path metadata missing package source",
    )

    run_command(
        [
            str(binary),
            "package",
            "add",
            "spectra-api",
            "--root",
            str(consumer),
            "--registry",
            str(registry),
            "--version",
            "0.1.0",
        ]
    )
    consumer_manifest = consumer.joinpath("spectra.toml").read_text(encoding="utf-8")
    require('[dependencies."spectra.api"]' in consumer_manifest, "consumer manifest must use canonical package key")
    lock_text = consumer.joinpath("spectra.lock").read_text(encoding="utf-8")
    require('name = "spectra.api"' in lock_text, "consumer lockfile missing spectra.api")
    require('compatibility = "spectralang-0.1"' in lock_text, "lockfile missing compatibility")

    installed = consumer / ".spectra" / "packages" / "spectra-api-0.1.0"
    require(installed.joinpath("spectra.toml").is_file(), "installed registry package missing manifest")

    for root in [ROOT / "packages" / "spectra-api", installed]:
        for command in ["build", "check", "run"]:
            run_command([str(binary), "package", command, "--root", str(root)], timeout=90)


def validate_planning_and_runner() -> None:
    roadmap = parse_toml(ROOT / "roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2217 = items.get("R-2217")
    require(r2217 is not None, "R-2217 missing from roadmap")
    require(r2217.get("status") == "complete", "R-2217 must be complete")
    require(r2217.get("owner") == "ecosystem", "R-2217 owner changed")
    require(r2217.get("dependencies") == ["R-2203", "R-2216"], "R-2217 dependencies changed")
    acceptance = "\n".join(r2217.get("acceptance", []))
    for term in [
        "spectralang package add spectra-api",
        "compatible Spectra and async runtime versions",
        "package build/check/run",
        "checksum and source path metadata",
        "validate_r2217_spectra_api_registry.py",
    ]:
        require(term in acceptance, f"R-2217 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2217 spectra.api Package Published to Local Registry", 1)[
        1
    ].split("## R-2218", 1)[0]
    for term in [
        "Status: `complete`",
        "spectralang package add spectra-api",
        "source_path",
        "scripts/validate_r2217_spectra_api_registry.py",
    ]:
        require(term in block, f"backlog R-2217 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2217` `spectra.api` package published to local registry (complete;" in plan,
        "implementation plan must mark R-2217 complete",
    )

    runner = read("run_tests.ps1")
    require("validate_r2217_spectra_api_registry.py" in runner, "run_tests.ps1 must run R-2217")
    require(
        'Teste = "validate_r2217_spectra_api_registry"' in runner,
        "run_tests.ps1 must record R-2217",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    default_binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    parser.add_argument("--binary", type=Path, default=default_binary)
    args = parser.parse_args()

    run_command([cargo_cmd(), "build", "-q", "-p", "spectra-cli"], timeout=120)
    validate_static_files()
    validate_registry_flow(args.binary.resolve())
    validate_planning_and_runner()
    print("validated R-2217 spectra.api local registry publishing")


if __name__ == "__main__":
    main()
