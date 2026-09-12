#!/usr/bin/env python3
"""R-3221 T1 — `spectra.agent` package distribution gate.

Automates what the item's manual verification did, end to end, against a
throwaway registry under `target/`:

  * the manifest shape (`packages/spectra-agent/spectra.toml`) and the
    bindings it publishes;
  * the crate's aggregation contract: an rlib only, no staticlib, no
    `#[no_mangle]` symbol, registered through `spectra_api::register`;
  * `spectralang package publish --root packages/spectra-agent --registry ...`
    and the registry metadata it writes (name, version, channel,
    compatibility, checksum, source path);
  * a consumer project that adds the package through `package add` and then
    builds, checks and runs — from the source root and from the installed
    copy, so the published bytes are consumable, not just present;
  * the roadmap registration of R-3221 under `phase_32`.

Nothing here touches the real local registry: the registry directory is
created under `target/r3221-agent-package/` and removed first.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE = "packages/spectra-agent"
WORK = ROOT / "target" / "r3221-agent-package"
REGISTRY = WORK / "registry"
CONSUMER = WORK / "consumer"
ROADMAP = "roadmap/roadmap.toml"
ITEM_ID = "R-3221"
PHASE_ID = "phase_32"

BINDINGS = [
    "src/bindings/mod.spectra",
    "src/bindings/agent.spectra",
    "src/bindings/tools.spectra",
    "src/bindings/journal.spectra",
]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3221 package validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def parse_toml(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


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


def run_command(args: list[str], timeout: int = 300) -> str:
    completed = subprocess.run(
        [str(arg) for arg in args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        timeout=timeout,
    )
    if completed.returncode != 0:
        fail(f"command {' '.join(str(arg) for arg in args)} failed:\n{completed.stdout}")
    return completed.stdout


def validate_manifest_shape() -> None:
    manifest = parse_toml(ROOT / PACKAGE / "spectra.toml")
    project = manifest.get("project", {})
    require(project.get("name") == "spectra.agent", "package name must be spectra.agent")
    require(project.get("version") == "0.1.0", "package version must be deterministic")
    require(
        project.get("entry") == "src/bindings/mod.spectra",
        "package entry must point at bindings/mod.spectra",
    )
    require(
        project.get("src_dirs") == ["src/bindings"],
        "the package must publish only the public bindings",
    )
    release = manifest.get("release", {})
    require(release.get("channel") == "beta", "manifest must declare the beta channel")
    require(
        release.get("compatibility") == "spectralang-0.1",
        "manifest must pin the compatible Spectra runtime level",
    )
    require(
        not manifest.get("dependencies"),
        "the distribution package must not depend on another Spectra package",
    )

    # The bindings the entry point publishes must exist and be parseable roots.
    root = read(f"{PACKAGE}/src/bindings/mod.spectra")
    require("module spectra_agent" in root, "binding root module name changed")
    require("public func main() returns int" in root, "package run must have an entry point")
    for binding in BINDINGS:
        path = ROOT / PACKAGE / binding
        require(path.is_file(), f"binding {binding} is missing")
    agent = read(f"{PACKAGE}/src/bindings/agent.spectra")
    for term in ["module std.agent", "public record AgentSpec", "public record Report"]:
        require(term in agent, f"bindings/agent.spectra missing {term}")


def validate_crate_aggregation() -> None:
    cargo = parse_toml(ROOT / PACKAGE / "Cargo.toml")
    require(cargo["package"]["name"] == "spectra-agent", "crate name changed")
    require(
        cargo["lib"]["crate-type"] == ["rlib"],
        "spectra-agent must be an rlib only: the namespace is aggregated by spectra-api",
    )
    for source in (ROOT / PACKAGE / "src").rglob("*.rs"):
        lines = source.read_text(encoding="utf-8", errors="replace").splitlines()
        exported = [line for line in lines if line.strip().startswith("#[no_mangle]")]
        require(
            not exported,
            f"{source.relative_to(ROOT)} must not export a linker symbol",
        )

    registration = read("packages/spectra-api/src/api_registration.rs")
    require(
        "spectra_agent::register()" in registration,
        "spectra-api registration must aggregate the agent namespace",
    )


def make_consumer() -> None:
    if WORK.exists():
        shutil.rmtree(WORK)
    source = CONSUMER / "src"
    source.mkdir(parents=True)
    # The consumer is the resolution/installation target of the published
    # package. Its own program is deliberately trivial: a package workspace
    # compiles its dependencies' entry modules into the same program, and the
    # published binding root declares the entry point `package run` requires
    # (the R-2217 convention), so two `main`s would collide. Consumption is
    # therefore proven where it is observable: the canonical dependency key in
    # the manifest, the resolved lockfile, the installed tree, and
    # build/check/run of the installed copy.
    source.joinpath("main.spectra").write_text(
        "\n".join(
            [
                "module spectra_agent_consumer",
                "",
                "public func main() returns int {",
                "    return 0",
                "}",
                "",
            ]
        ),
        encoding="utf-8",
    )
    CONSUMER.joinpath("spectra.toml").write_text(
        "\n".join(
            [
                "[project]",
                'name = "spectra_agent_consumer"',
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


def validate_registry_flow(binary: Path) -> None:
    make_consumer()

    run_command(
        [
            binary,
            "package",
            "publish",
            "--root",
            PACKAGE,
            "--registry",
            REGISTRY,
        ]
    )
    metadata_path = REGISTRY / "spectra.agent" / "0.1.0" / "package.toml"
    require(metadata_path.is_file(), f"registry metadata {metadata_path} is missing")
    metadata = parse_toml(metadata_path)
    require(metadata.get("name") == "spectra.agent", "registry metadata has the wrong name")
    require(metadata.get("version") == "0.1.0", "registry metadata has the wrong version")
    require(metadata.get("channel") == "beta", "registry metadata is missing the channel")
    require(
        metadata.get("compatibility") == "spectralang-0.1",
        "registry metadata is missing the compatibility level",
    )
    require(
        isinstance(metadata.get("checksum"), str) and metadata["checksum"],
        "registry metadata is missing the checksum",
    )
    require(
        PACKAGE in str(metadata.get("source_path", "")).replace("\\", "/"),
        "registry metadata is missing the source path",
    )

    run_command(
        [
            binary,
            "package",
            "add",
            "spectra-agent",
            "--root",
            CONSUMER,
            "--registry",
            REGISTRY,
            "--version",
            "0.1.0",
        ]
    )
    consumer_manifest = (CONSUMER / "spectra.toml").read_text(encoding="utf-8")
    require(
        '[dependencies."spectra.agent"]' in consumer_manifest,
        "the consumer manifest must use the canonical package key",
    )
    lock = (CONSUMER / "spectra.lock").read_text(encoding="utf-8")
    require('name = "spectra.agent"' in lock, "the consumer lockfile is missing spectra.agent")
    require(
        'compatibility = "spectralang-0.1"' in lock,
        "the consumer lockfile is missing the compatibility level",
    )

    installed = CONSUMER / ".spectra" / "packages" / "spectra-agent-0.1.0"
    require(
        (installed / "spectra.toml").is_file(),
        "the installed registry package is missing its manifest",
    )
    for binding in BINDINGS:
        require(
            (installed / binding).is_file(),
            f"the installed package is missing {binding}",
        )
        require(
            (installed / binding).read_bytes() == (ROOT / PACKAGE / binding).read_bytes(),
            f"the installed {binding} differs from the published source",
        )

    # The published copy and the source root are each a complete workspace that
    # builds, checks and runs. (The consumer's own program is not built: see
    # `make_consumer` for why the R-2217 flow verifies the package roots.)
    for root in [ROOT / PACKAGE, installed]:
        for command in ["build", "check", "run"]:
            run_command([binary, "package", command, "--root", root], timeout=600)


def validate_tracker() -> None:
    tracker = parse_toml(ROOT / ROADMAP)
    phases = {phase.get("id") for phase in tracker.get("phases", [])}
    require(PHASE_ID in phases, f"{ROADMAP} does not register phase '{PHASE_ID}'")
    items = {item.get("id"): item for item in tracker.get("items", [])}
    require(ITEM_ID in items, f"{ROADMAP} does not register item '{ITEM_ID}'")
    require(
        items[ITEM_ID].get("phase") == PHASE_ID,
        f"{ITEM_ID} is not registered under '{PHASE_ID}'",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    default_binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    parser.add_argument("--binary", type=Path, default=default_binary)
    arguments = parser.parse_args()

    run_command([cargo_cmd(), "build", "-q", "-p", "spectra-cli", "--offline"], timeout=1200)
    validate_manifest_shape()
    validate_crate_aggregation()
    validate_registry_flow(arguments.binary.resolve())
    validate_tracker()
    print(
        "validated R-3221 spectra.agent package (manifest, publish, consumer add, "
        "build/check/run)"
    )


if __name__ == "__main__":
    main()
