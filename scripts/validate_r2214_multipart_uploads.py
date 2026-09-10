from __future__ import annotations

import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    source = ROOT / path
    if source.suffix == ".rs":
        return "\n".join(
            sibling.read_text(encoding="utf-8")
            for sibling in sorted(source.parent.rglob("*.rs"))
        )
    return source.read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2214 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


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
    multipart = read("packages/spectra-api/src/multipart.rs")
    for term in [
        "pub struct Multipart",
        "pub struct MultipartPart",
        "pub struct MultipartLimits",
        "MultipartErrorKind::TooManyParts",
        "MultipartErrorKind::TotalTooLarge",
        "MultipartErrorKind::PartTooLarge",
        "MultipartErrorKind::MissingOpeningBoundary",
        "MultipartErrorKind::MissingHeaderTerminator",
        "MultipartErrorKind::MissingName",
        "MultipartErrorKind::InvalidUtf8",
        "spool_file",
        "file_read",
        "file_spool_to",
        "parses_text_and_file_parts_with_spooled_file_reader",
        "enforces_total_part_count_and_per_part_limits",
        "rejects_malformed_multipart_bodies",
    ]:
        require(term in multipart, f"multipart.rs missing {term}")
    require(
        "std::env::temp_dir()" in multipart and "spectra-api-multipart" in multipart,
        "multipart files must be spooled to a managed temp directory",
    )
    require(
        "content.len() > limits.max_part_bytes" in multipart,
        "multipart parser must enforce per-part size limits",
    )

    lib = read("packages/spectra-api/src/lib.rs")
    runtime = read("packages/spectra-api/src/host_calls.rs")
    midend = read("midend/src/lowering.rs")
    builtins = read("compiler/src/semantic/builtin_modules.rs")
    semantic = read("compiler/src/semantic/mod.rs")
    for name in [
        "spectra.api.multipart.parse",
        "spectra.api.multipart.part_count",
        "spectra.api.multipart.field_count",
        "spectra.api.multipart.file_count",
        "spectra.api.multipart.text",
        "spectra.api.multipart.part",
        "spectra.api.multipart.part_name",
        "spectra.api.multipart.part_filename",
        "spectra.api.multipart.part_content_type",
        "spectra.api.multipart.part_size",
        "spectra.api.multipart.part_is_file",
        "spectra.api.multipart.file_path",
        "spectra.api.multipart.file_read",
        "spectra.api.multipart.file_spool_to",
        "spectra.api.multipart.error_code",
        "spectra.api.multipart.error_message",
    ]:
        require(name in lib, f"{name} missing from host-call table")
        require(name in runtime, f"{name} missing from runtime contract")
    for term in [
        '"multipart", "parse"',
        '"multipart", "file_read"',
        '"multipart", "file_spool_to"',
        "MultipartPart",
        "Multipart",
    ]:
        require(term in midend, f"midend missing {term}")
    for term in [
        "std.api.multipart",
        "std.api.multipart.Multipart",
        "std.api.multipart.MultipartPart",
        "std.api.multipart.parse",
        "std.api.multipart.file_read",
        "std.api.multipart.file_spool_to",
    ]:
        require(term in builtins, f"builtin surface missing {term}")
    require("std.api.multipart" in semantic, "semantic namespace seed missing std.api.multipart")


def validate_fixture_and_docs() -> None:
    fixture = read("tests/validation/138_api_multipart_uploads.spectra")
    for term in [
        "std.api.multipart",
        "Content-Disposition: form-data",
        "filename=\\\"hello.txt\\\"",
        "filename=\\\"second.txt\\\"",
        "part_count(form)",
        "field_count(form)",
        "file_count(form)",
        "text(form, \"title\", 0)",
        "file_read",
        "file_spool_to",
        "too_many",
        "too_big",
        "error_message",
    ]:
        require(term in fixture, f"fixture missing {term}")

    docs = read("docs/api/std-api-multipart.md")
    for term in [
        "std.api.multipart",
        "multipart/form-data",
        "MultipartPart",
        "max_total_bytes",
        "max_parts",
        "max_part_bytes",
        "file_read",
        "file_spool_to",
        "tests/validation/138_api_multipart_uploads.spectra",
    ]:
        require(term in docs, f"multipart docs missing {term}")

    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    for term in [
        'module std.api.multipart',
        "type std.api.multipart.Multipart",
        "type std.api.multipart.MultipartPart",
        "func std.api.multipart.parse",
        "func std.api.multipart.file_read",
    ]:
        require(term in snapshot, f"std.api snapshot missing {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2214 = items.get("R-2214")
    require(r2214 is not None, "R-2214 missing from roadmap")
    require(r2214.get("status") == "complete", "R-2214 must be marked complete")
    require(r2214.get("owner") == "web", "R-2214 owner must remain web")
    require(r2214.get("dependencies") == ["R-2213"], "R-2214 dependencies changed")
    acceptance = "\n".join(r2214.get("acceptance", []))
    for term in [
        "std.api.multipart",
        "MultipartPart",
        "stream-friendly file readers",
        "per-request size limits",
        "per-part count limits",
        "spooled",
        "138_api_multipart_uploads.spectra",
        "scripts/validate_r2214_multipart_uploads.py",
    ]:
        require(term in acceptance, f"R-2214 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2214 Multipart Form and File Uploads", 1)[1].split(
        "## R-2215", 1
    )[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/multipart.rs",
        "std.api.multipart",
        "MultipartPart",
        "138_api_multipart_uploads.spectra",
        "validate_r2214_multipart_uploads.py",
    ]:
        require(term in block, f"backlog R-2214 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2214` Multipart form and file uploads (complete;" in plan,
        "implementation plan must mark R-2214 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2214_multipart_uploads.py" in runner, "run_tests.ps1 must run R-2214")
    require(
        'Teste = "validate_r2214_multipart_uploads"' in runner,
        "run_tests.ps1 must record R-2214",
    )


def validate_commands() -> None:
    binary = ROOT / "target" / "debug" / ("spectralang.exe" if sys.platform.startswith("win") else "spectralang")
    run_command(["cargo", "test", "-q", "-p", "spectra-api", "multipart", "--offline"])
    run_command(["cargo", "test", "-q", "-p", "spectra-compiler", "--offline"])
    run_command(["cargo", "test", "-q", "-p", "spectra-midend", "--offline"])
    run_command(["cargo", "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "compile", "tests/validation/138_api_multipart_uploads.spectra"])
    run_command([str(binary), "run", "tests/validation/138_api_multipart_uploads.spectra"])


def main() -> None:
    validate_implementation()
    validate_fixture_and_docs()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2214 multipart form and file uploads")


if __name__ == "__main__":
    main()
