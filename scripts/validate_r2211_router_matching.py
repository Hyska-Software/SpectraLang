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
    print(f"R-2211 validation failed: {message}", file=sys.stderr)
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


def validate_router_implementation() -> None:
    routing = read("packages/spectra-api/src/routing.rs")
    lib = read("packages/spectra-api/src/lib.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    midend = read("midend/src/lowering.rs")
    builtins = read("compiler/src/semantic/builtin_modules.rs")

    for term in [
        "pub struct Router",
        "pub struct Route",
        "pub struct RouteMatch",
        "pub struct RouteConflict",
        "enum RouteSegment",
        "RouteSegment::Literal",
        "RouteSegment::Param",
        "RouteSegment::Wildcard",
        "parse_pattern",
        "detect_conflict",
        "regex_like_matches",
        "benchmark_100k_lookup",
        "one_hundred_thousand_routes_lookup_is_sub_millisecond",
    ]:
        require(term in routing, f"routing.rs missing {term}")

    for host in [
        "spectra.api.routing.route_add",
        "spectra.api.routing.get",
        "spectra.api.routing.post",
        "spectra.api.routing.route_match",
        "spectra.api.routing.match_param",
        "spectra.api.routing.match_param_int",
        "spectra.api.routing.last_conflict",
    ]:
        require(host in lib, f"{host} missing from spectra-api host table")
        require(host in runtime_api, f"{host} missing from runtime API contract")
        require(host in midend, f"{host} missing from midend lowering")

    for function in [
        "std.api.routing.route_add",
        "std.api.routing.route_match",
        "std.api.routing.match_route_id",
        "std.api.routing.match_param",
        "std.api.routing.match_param_int",
        "std.api.routing.last_conflict",
    ]:
        require(function in builtins, f"{function} missing from builtin surface")
    require("std.api.routing.RouteMatch" in builtins, "RouteMatch missing from public type table")


def validate_fixtures_and_docs() -> None:
    fixture = "tests/validation/135_api_router_matching.spectra"
    require((ROOT / fixture).is_file(), f"missing fixture {fixture}")
    source = read(fixture)
    for term in [
        "/users",
        "/users/{id}",
        "/files/*path",
        "/orders/{id:\\\\d+}",
        "match_param_int",
        "last_conflict",
    ]:
        require(term in source, f"fixture missing {term}")

    docs = read("docs/api/std-api-routing.md")
    for term in [
        "std.api.routing",
        "/users/{id}",
        "/files/*path",
        "/orders/{id:\\d+}",
        "route_match",
        "match_param_int",
        "conflict",
        "scripts/validate_r2211_router_matching.py",
    ]:
        require(term in docs, f"docs missing {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2211 = items.get("R-2211")
    require(r2211 is not None, "R-2211 missing from roadmap")
    require(r2211.get("status") == "complete", "R-2211 must be marked complete")
    require(r2211.get("owner") == "web", "R-2211 owner must remain web")
    require(r2211.get("dependencies") == ["R-2210"], "R-2211 dependencies changed")
    acceptance = "\n".join(r2211.get("acceptance", []))
    for term in [
        "/users/{id}",
        "/files/*path",
        "/orders/{id:\\d+}",
        "path parameters",
        "conflicting paths",
        "100k registered routes with sub-millisecond lookup",
        "tests/validation/135_api_router_matching.spectra",
        "scripts/validate_r2211_router_matching.py",
    ]:
        require(term in acceptance, f"R-2211 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2211 Router: Path Matching and Wildcards", 1)[1].split("## R-2212", 1)[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/routing.rs",
        "docs/api/std-api-routing.md",
        "validate_r2211_router_matching.py",
    ]:
        require(term in block, f"backlog R-2211 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2211` Router: path matching, params, wildcards (complete;" in plan,
        "implementation plan must mark R-2211 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2211_router_matching.py" in runner, "run_tests.ps1 must run R-2211")
    require('Teste = "validate_r2211_router_matching"' in runner, "run_tests.ps1 must record R-2211")


def run_regressions() -> None:
    run_command([CARGO, "test", "-q", "-p", "spectra-api", "routing", "--offline"])
    run_command([CARGO, "test", "-q", "-p", "spectra-compiler", "--offline"])
    run_command([CARGO, "test", "-q", "-p", "spectra-midend", "--offline"])
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(SPECTRALANG), "compile", "tests/validation/135_api_router_matching.spectra"])
    run_command([str(SPECTRALANG), "run", "tests/validation/135_api_router_matching.spectra"])


def main() -> None:
    validate_router_implementation()
    validate_fixtures_and_docs()
    run_regressions()
    validate_planning()
    validate_runner()
    print("validated R-2211 router path matching and wildcards")


if __name__ == "__main__":
    main()
