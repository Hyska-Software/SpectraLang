from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REQUIRED_MODULES = [
    "std.api",
    "std.api.http",
    "std.api.server",
    "std.api.client",
    "std.api.json",
    "std.api.tls",
    "std.api.routing",
    "std.api.query",
    "std.api.form",
    "std.api.multipart",
    "std.api.handler",
    "std.api.cors",
    "std.api.errors",
]
REQUIRED_FUNCTIONS = [
    "std.api.http.method_name",
    "std.api.http.method_allows_body",
    "std.api.http.method_is_safe",
    "std.api.http.method_get",
    "std.api.http.method_head",
    "std.api.http.method_post",
    "std.api.http.method_put",
    "std.api.http.method_patch",
    "std.api.http.method_delete",
    "std.api.http.method_options",
    "std.api.http.status_reason",
    "std.api.http.status_class",
    "std.api.http.status_is_success",
    "std.api.http.status_continue",
    "std.api.http.status_switching_protocols",
    "std.api.http.status_ok",
    "std.api.http.status_created",
    "std.api.http.status_accepted",
    "std.api.http.status_no_content",
    "std.api.http.status_moved_permanently",
    "std.api.http.status_found",
    "std.api.http.status_not_modified",
    "std.api.http.status_bad_request",
    "std.api.http.status_unauthorized",
    "std.api.http.status_forbidden",
    "std.api.http.status_not_found",
    "std.api.http.status_method_not_allowed",
    "std.api.http.status_conflict",
    "std.api.http.status_unsupported_media_type",
    "std.api.http.status_unprocessable_content",
    "std.api.http.status_too_many_requests",
    "std.api.http.status_internal_server_error",
    "std.api.http.status_bad_gateway",
    "std.api.http.status_service_unavailable",
    "std.api.http.status_gateway_timeout",
    "std.api.http.header_name_is_valid",
    "std.api.http.header_value_is_valid",
    "std.api.http.request",
    "std.api.http.request_new",
    "std.api.http.request_method",
    "std.api.http.request_path",
    "std.api.http.request_header",
    "std.api.http.request_with_header",
    "std.api.http.request_cookie",
    "std.api.http.response",
    "std.api.http.response_new",
    "std.api.http.response_status",
    "std.api.http.response_header",
    "std.api.http.response_body_len",
    "std.api.http.header",
    "std.api.http.header_name",
    "std.api.http.header_value",
    "std.api.http.cookie",
    "std.api.http.cookie_name",
    "std.api.http.cookie_value",
    "std.api.http.status",
    "std.api.server.new",
    "std.api.server.listen",
    "std.api.server.serve",
    "std.api.server.state",
    "std.api.server.shutdown",
    "std.api.server.local_port",
    "std.api.server.signal",
    "std.api.server.stats",
    "std.api.client.new",
    "std.api.client.request",
    "std.api.client.timeout_ms",
    "std.api.json.validate",
    "std.api.json.kind",
    "std.api.json.encode",
    "std.api.json.decode",
    "std.api.tls.config_new",
    "std.api.tls.config_mode",
    "std.api.tls.server_config",
    "std.api.tls.client_config",
    "std.api.routing.router",
    "std.api.routing.router_new",
    "std.api.routing.route_count",
    "std.api.routing.route_id",
    "std.api.routing.route_add",
    "std.api.routing.route_match",
    "std.api.routing.match_route_id",
    "std.api.routing.match_param",
    "std.api.routing.match_param_int",
    "std.api.routing.last_conflict",
    "std.api.routing.get",
    "std.api.routing.post",
    "std.api.routing.put",
    "std.api.routing.patch",
    "std.api.routing.delete",
    "std.api.query.type_string",
    "std.api.query.type_int",
    "std.api.query.type_bool",
    "std.api.query.parse",
    "std.api.query.len",
    "std.api.query.has",
    "std.api.query.count",
    "std.api.query.first",
    "std.api.query.value",
    "std.api.query.int",
    "std.api.query.bool",
    "std.api.query.schema",
    "std.api.query.schema_field",
    "std.api.query.bind",
    "std.api.query.binding_ok",
    "std.api.query.binding_error",
    "std.api.query.binding_count",
    "std.api.query.binding_value",
    "std.api.query.binding_int",
    "std.api.query.binding_bool",
    "std.api.query.error_code",
    "std.api.query.error_message",
    "std.api.form.type_string",
    "std.api.form.type_int",
    "std.api.form.type_bool",
    "std.api.form.parse",
    "std.api.form.len",
    "std.api.form.has",
    "std.api.form.count",
    "std.api.form.first",
    "std.api.form.value",
    "std.api.form.int",
    "std.api.form.bool",
    "std.api.form.schema",
    "std.api.form.schema_field",
    "std.api.form.bind",
    "std.api.form.binding_ok",
    "std.api.form.binding_error",
    "std.api.form.binding_count",
    "std.api.form.binding_value",
    "std.api.form.binding_int",
    "std.api.form.binding_bool",
    "std.api.form.error_code",
    "std.api.form.error_message",
    "std.api.multipart.parse",
    "std.api.multipart.part_count",
    "std.api.multipart.field_count",
    "std.api.multipart.file_count",
    "std.api.multipart.text",
    "std.api.multipart.part",
    "std.api.multipart.part_name",
    "std.api.multipart.part_filename",
    "std.api.multipart.part_content_type",
    "std.api.multipart.part_size",
    "std.api.multipart.part_is_file",
    "std.api.multipart.file_path",
    "std.api.multipart.file_read",
    "std.api.multipart.file_spool_to",
    "std.api.multipart.error_code",
    "std.api.multipart.error_message",
    "std.api.handler.text",
    "std.api.handler.json",
    "std.api.handler.bytes",
    "std.api.handler.status",
    "std.api.handler.with_header",
    "std.api.handler.into_response",
    "std.api.handler.into_text_response",
    "std.api.handler.into_status_response",
    "std.api.handler.error",
    "std.api.handler.error_response",
    "std.api.handler.error_code",
    "std.api.handler.error_message",
    "std.api.handler.last_error_message",
    "std.api.handler.register_sync",
    "std.api.handler.register_async",
    "std.api.handler.register_sync_callback",
    "std.api.handler.register_async_callback",
    "std.api.handler.dispatch_sync",
    "std.api.handler.dispatch_async",
    "std.api.cors.policy",
    "std.api.cors.permissive",
    "std.api.cors.allow_origin",
    "std.api.cors.allow_method",
    "std.api.cors.allow_header",
    "std.api.cors.expose_header",
    "std.api.cors.allow_credentials",
    "std.api.cors.max_age",
    "std.api.cors.middleware",
    "std.api.cors.is_preflight",
    "std.api.cors.preflight",
    "std.api.cors.apply",
    "std.api.cors.allowed_origin",
    "std.api.errors.last_code",
    "std.api.errors.last_message",
]
REQUIRED_TYPES = [
    "std.api.http.Request",
    "std.api.http.Response",
    "std.api.http.Method",
    "std.api.http.Status",
    "std.api.http.Header",
    "std.api.http.Headers",
    "std.api.http.Cookie",
    "std.api.http.Body",
    "std.api.server.Server",
    "std.api.client.Client",
    "std.api.json.JsonValue",
    "std.api.tls.TlsConfig",
    "std.api.routing.Route",
    "std.api.routing.Router",
    "std.api.routing.RouteMatch",
    "std.api.query.Query",
    "std.api.query.QuerySchema",
    "std.api.query.QueryBinding",
    "std.api.form.Form",
    "std.api.form.FormSchema",
    "std.api.form.FormBinding",
    "std.api.multipart.Multipart",
    "std.api.multipart.MultipartPart",
    "std.api.handler.HandlerHandle",
    "std.api.handler.AsyncHandlerHandle",
    "std.api.handler.HandlerError",
    "std.api.cors.CorsPolicy",
    "std.api.errors.ApiError",
]


def read(path: str) -> str:
    source = ROOT / path
    if source.suffix == ".rs":
        return "\n".join(
            sibling.read_text(encoding="utf-8")
            for sibling in sorted(source.parent.rglob("*.rs"))
        )
    return source.read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2203 validation failed: {message}", file=sys.stderr)
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


def validate_public_surface() -> None:
    builtins = read("compiler/src/semantic/builtin_modules.rs")
    semantic = read("compiler/src/semantic/mod.rs")
    lsp = read("tools/spectra-lsp/src/main.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    fixture = read("tests/semantic/std_api_surface.spectra")

    for module in REQUIRED_MODULES:
        require(module in builtins, f"{module} missing from builtin surface")
        require(module in semantic, f"{module} missing from semantic namespace seed")
        require(f"module {module}" in snapshot, f"{module} missing from snapshot")

    for function in REQUIRED_FUNCTIONS:
        require(function in builtins, f"{function} missing from builtin function constants")
        require(function in snapshot, f"{function} missing from function table snapshot")

    for ty in REQUIRED_TYPES:
        require(ty in builtins, f"{ty} missing from builtin type constants")
        require(ty in snapshot, f"{ty} missing from type table snapshot")

    for sample_call in [
        "from std.api.http",
        "from std.api.json",
        "from std.api.tls",
        "from std.api.routing",
        "from std.api.query",
        "std.api.form",
        "std.api.multipart",
        "std.api.handler",
        "from std.api.errors",
        "request_new",
        "method_name",
        "client_config",
        "router_new",
        "parse",
        "api_form.len",
        "multipart.part_count",
        "api_handler.text",
        "last_code",
    ]:
        require(sample_call in fixture, f"{sample_call} missing from semantic fixture")

    for trait_name in ["IntoResponse", "Handler", "AsyncHandler"]:
        require(trait_name in builtins, f"{trait_name} trait missing from builtin handler surface")

    require(
        "STD_API_PUBLIC_FUNCTIONS" in lsp and "std_api_completion_items" in lsp,
        "LSP completion must be driven by the std.api public function table",
    )
    require(
        "std_api_surface_resolves_qualified_and_aliased_calls" in read("compiler/tests/stage_smoke.rs"),
        "compiler semantic regression test missing",
    )
    require(
        "std_api_public_function_table_is_snapshotted" in read("compiler/tests/snapshot_tests.rs"),
        "public function table snapshot test missing",
    )
    require(
        "std_api_completion_items_cover_modules_types_and_functions" in lsp,
        "LSP completion regression test missing",
    )


def validate_cli(binary: Path) -> None:
    binary_arg = str(binary)
    run_command([binary_arg, "check", "tests/semantic/std_api_surface.spectra"])
    run_command([binary_arg, "fmt", "--check", "tests/semantic/std_api_surface.spectra"])
    experimental = run_command([binary_arg, "--list-experimental"])
    require(
        "Experimental language features: none" in experimental,
        "--list-experimental must keep reporting no active syntax gates",
    )
    require(
        "std.api" not in experimental,
        "std.api must be stable surface, not an experimental syntax gate",
    )


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2203 = items.get("R-2203")
    require(r2203 is not None, "R-2203 missing from roadmap")
    require(r2203.get("status") == "complete", "R-2203 must be marked complete")
    require(r2203.get("owner") == "semantic", "R-2203 owner must remain semantic")
    require(r2203.get("dependencies") == ["R-2202"], "R-2203 must depend only on R-2202")
    acceptance = "\n".join(r2203.get("acceptance", []))
    for term in [
        "std.api.*",
        "formatter",
        "LSP completion",
        "--list-experimental",
        "qualified std.api.* calls",
        "snapshot tests",
        "scripts/validate_r2203_std_api_surface.py",
    ]:
        require(term in acceptance, f"R-2203 acceptance must mention {term}")

    backlog = read("docs/roadmap-backlog.md")
    require("## R-2203 std.api Surface in Semantic Analysis" in backlog, "backlog R-2203 missing")
    r2203_block = backlog.split("## R-2203 std.api Surface in Semantic Analysis", 1)[1].split("## R-2204", 1)[0]
    for term in [
        "Status: `complete`",
        "compiler/src/semantic/builtin_modules.rs",
        "tests/semantic/std_api_surface.spectra",
        "std_api_public_function_table.snap",
        "validate_r2203_std_api_surface.py",
    ]:
        require(term in r2203_block, f"backlog R-2203 block must mention {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2203` `std.api.*` semantic and tooling surface (complete;" in plan,
        "implementation plan must mark R-2203 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require(
        "validate_r2203_std_api_surface.py" in runner,
        "run_tests.ps1 must run R-2203 validator",
    )
    require(
        'Teste = "validate_r2203_std_api_surface"' in runner,
        "run_tests.ps1 must record R-2203 result",
    )


def main() -> None:
    binary = ROOT / "target" / "debug" / ("spectralang.exe" if sys.platform.startswith("win") else "spectralang")
    if not binary.exists():
        fail(f"missing CLI binary {binary}; run cargo build -p spectra-cli first")
    validate_public_surface()
    validate_cli(binary)
    validate_planning()
    validate_runner()
    print("validated R-2203 std.api semantic and tooling surface")


if __name__ == "__main__":
    main()
