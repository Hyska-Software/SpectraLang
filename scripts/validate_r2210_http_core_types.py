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
    print(f"R-2210 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_command(args: list[str]) -> str:
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
    return completed.stdout


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


def validate_rust_surface() -> None:
    http = read("packages/spectra-api/src/http.rs")
    lib = read("packages/spectra-api/src/lib.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    midend = read("midend/src/lowering.rs")
    builtins = read("compiler/src/semantic/builtin_modules.rs")

    for term in [
        "pub enum Method",
        "pub struct Status",
        "pub struct Request",
        "pub struct Response",
        "pub struct Headers",
        "pub struct Cookie",
        "Header::new",
        "Cookie::new",
        "eq_ignore_ascii_case",
        "cookie_value_from_header",
        "method_and_status_enumerate_documented_http_values",
        "headers_and_cookies_are_case_insensitive_and_validate_input",
        "request_response_types_cover_crud_style_flow",
    ]:
        require(term in http, f"http.rs missing {term}")

    for host in [
        "spectra.api.http.method_get",
        "spectra.api.http.method_post",
        "spectra.api.http.status_created",
        "spectra.api.http.request",
        "spectra.api.http.request_path",
        "spectra.api.http.response",
        "spectra.api.http.header_name",
        "spectra.api.http.cookie_value",
    ]:
        require(host in lib, f"host call {host} missing from spectra-api")
        require(host in runtime_api, f"host call {host} missing from runtime API contract")
        require(host in midend, f"host call {host} missing from midend lowering")

    for function in [
        "std.api.http.method_get",
        "std.api.http.status_created",
        "std.api.http.request_path",
        "std.api.http.response_body_len",
        "std.api.http.cookie_value",
    ]:
        require(function in builtins, f"{function} missing from builtin surface")

    require("is_std_api_handle_type_segments" in midend, "midend must lower API handle types")


def validate_fixtures_and_docs() -> None:
    fixture = "tests/validation/134_http_core_types.spectra"
    require((ROOT / fixture).is_file(), f"missing fixture {fixture}")
    source = read(fixture)
    for term in [
        "Request",
        "Response",
        "func create_user(req: Request) returns Response",
        "method_post()",
        "status_created()",
        "header(",
        "cookie(",
    ]:
        require(term in source, f"fixture missing {term}")

    docs = read("docs/api/std-api-http-types.md")
    for term in [
        "std.api.http",
        "Request",
        "Response",
        "method_get()",
        "status_created()",
        "case-insensitive",
        "scripts/validate_r2210_http_core_types.py",
    ]:
        require(term in docs, f"docs missing {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2210 = items.get("R-2210")
    require(r2210 is not None, "R-2210 missing from roadmap")
    require(r2210.get("status") == "complete", "R-2210 must be marked complete")
    require(r2210.get("owner") == "web", "R-2210 owner must remain web")
    require(r2210.get("dependencies") == ["R-2204"], "R-2210 dependencies changed")
    acceptance = "\n".join(r2210.get("acceptance", []))
    for term in [
        "handler parameters and return values",
        "Method and Status documented values",
        "case-insensitive and validate input",
        "representative CRUD request/response flows",
        "tests/validation/134_http_core_types.spectra",
        "scripts/validate_r2210_http_core_types.py",
    ]:
        require(term in acceptance, f"R-2210 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2210 Request, Response, Header, Cookie, Method, Status Types", 1)[1].split("## R-2211", 1)[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/http.rs",
        "docs/api/std-api-http-types.md",
        "validate_r2210_http_core_types.py",
    ]:
        require(term in block, f"backlog R-2210 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2210` `Request`, `Response`, `Header`, `Cookie`, `Method`, `Status` (complete;"
        in plan,
        "implementation plan must mark R-2210 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2210_http_core_types.py" in runner, "run_tests.ps1 must run R-2210")
    require('Teste = "validate_r2210_http_core_types"' in runner, "run_tests.ps1 must record R-2210")


def run_regressions() -> None:
    run_command([CARGO, "test", "-q", "-p", "spectra-api", "--offline"])
    run_command([CARGO, "test", "-q", "-p", "spectra-compiler", "--offline"])
    run_command([CARGO, "test", "-q", "-p", "spectra-midend", "--offline"])
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(SPECTRALANG), "compile", "tests/validation/134_http_core_types.spectra"])
    run_command([str(SPECTRALANG), "run", "tests/validation/134_http_core_types.spectra"])


def main() -> None:
    validate_rust_surface()
    validate_fixtures_and_docs()
    run_regressions()
    validate_planning()
    validate_runner()
    print("validated R-2210 HTTP core types")


if __name__ == "__main__":
    main()
