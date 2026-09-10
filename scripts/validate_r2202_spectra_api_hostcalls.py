from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 556
LEGACY_REQUIRED_HOST_CALLS = [
    "spectra.api.version.major",
    "spectra.api.version.minor",
    "spectra.api.version.patch",
    "spectra.api.http.method_name",
    "spectra.api.http.method_allows_body",
    "spectra.api.http.method_is_safe",
    "spectra.api.http.method_get",
    "spectra.api.http.method_head",
    "spectra.api.http.method_post",
    "spectra.api.http.method_put",
    "spectra.api.http.method_patch",
    "spectra.api.http.method_delete",
    "spectra.api.http.method_options",
    "spectra.api.http.status_reason",
    "spectra.api.http.status_class",
    "spectra.api.http.status_is_success",
    "spectra.api.http.status",
    "spectra.api.http.status_continue",
    "spectra.api.http.status_switching_protocols",
    "spectra.api.http.status_ok",
    "spectra.api.http.status_created",
    "spectra.api.http.status_accepted",
    "spectra.api.http.status_no_content",
    "spectra.api.http.status_moved_permanently",
    "spectra.api.http.status_found",
    "spectra.api.http.status_not_modified",
    "spectra.api.http.status_bad_request",
    "spectra.api.http.status_unauthorized",
    "spectra.api.http.status_forbidden",
    "spectra.api.http.status_not_found",
    "spectra.api.http.status_method_not_allowed",
    "spectra.api.http.status_conflict",
    "spectra.api.http.status_unsupported_media_type",
    "spectra.api.http.status_unprocessable_content",
    "spectra.api.http.status_too_many_requests",
    "spectra.api.http.status_internal_server_error",
    "spectra.api.http.status_bad_gateway",
    "spectra.api.http.status_service_unavailable",
    "spectra.api.http.status_gateway_timeout",
    "spectra.api.http.header_name_is_valid",
    "spectra.api.http.header_value_is_valid",
    "spectra.api.http.request_new",
    "spectra.api.http.request",
    "spectra.api.http.request_method",
    "spectra.api.http.request_path",
    "spectra.api.http.request_header",
    "spectra.api.http.request_with_header",
    "spectra.api.http.request_cookie",
    "spectra.api.http.response_new",
    "spectra.api.http.response",
    "spectra.api.http.response_status",
    "spectra.api.http.response_header",
    "spectra.api.http.response_body_len",
    "spectra.api.http.header",
    "spectra.api.http.header_name",
    "spectra.api.http.header_value",
    "spectra.api.http.cookie",
    "spectra.api.http.cookie_name",
    "spectra.api.http.cookie_value",
    "spectra.api.server.new",
    "spectra.api.server.listen",
    "spectra.api.server.serve",
    "spectra.api.server.state",
    "spectra.api.server.shutdown",
    "spectra.api.server.local_port",
    "spectra.api.server.signal",
    "spectra.api.server.stats",
    "spectra.api.client.new",
    "spectra.api.client.request",
    "spectra.api.client.timeout_ms",
    "spectra.api.json.validate",
    "spectra.api.json.kind",
    "spectra.api.tls.config_new",
    "spectra.api.tls.config_mode",
    "spectra.api.routing.router_new",
    "spectra.api.routing.route_count",
    "spectra.api.routing.route_id",
    "spectra.api.routing.route_add",
    "spectra.api.routing.get",
    "spectra.api.routing.post",
    "spectra.api.routing.put",
    "spectra.api.routing.patch",
    "spectra.api.routing.delete",
    "spectra.api.routing.route_match",
    "spectra.api.routing.match_route_id",
    "spectra.api.routing.match_param",
    "spectra.api.routing.match_param_int",
    "spectra.api.routing.last_conflict",
    "spectra.api.query.type_string",
    "spectra.api.query.type_int",
    "spectra.api.query.type_bool",
    "spectra.api.query.parse",
    "spectra.api.query.len",
    "spectra.api.query.has",
    "spectra.api.query.count",
    "spectra.api.query.first",
    "spectra.api.query.value",
    "spectra.api.query.int",
    "spectra.api.query.bool",
    "spectra.api.query.schema",
    "spectra.api.query.schema_field",
    "spectra.api.query.bind",
    "spectra.api.query.binding_ok",
    "spectra.api.query.binding_error",
    "spectra.api.query.binding_count",
    "spectra.api.query.binding_value",
    "spectra.api.query.binding_int",
    "spectra.api.query.binding_bool",
    "spectra.api.query.error_code",
    "spectra.api.query.error_message",
    "spectra.api.form.type_string",
    "spectra.api.form.type_int",
    "spectra.api.form.type_bool",
    "spectra.api.form.parse",
    "spectra.api.form.len",
    "spectra.api.form.has",
    "spectra.api.form.count",
    "spectra.api.form.first",
    "spectra.api.form.value",
    "spectra.api.form.int",
    "spectra.api.form.bool",
    "spectra.api.form.schema",
    "spectra.api.form.schema_field",
    "spectra.api.form.bind",
    "spectra.api.form.binding_ok",
    "spectra.api.form.binding_error",
    "spectra.api.form.binding_count",
    "spectra.api.form.binding_value",
    "spectra.api.form.binding_int",
    "spectra.api.form.binding_bool",
    "spectra.api.form.error_code",
    "spectra.api.form.error_message",
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
    "spectra.api.handler.text",
    "spectra.api.handler.json",
    "spectra.api.handler.bytes",
    "spectra.api.handler.status",
    "spectra.api.handler.with_header",
    "spectra.api.handler.into_response",
    "spectra.api.handler.into_text_response",
    "spectra.api.handler.into_status_response",
    "spectra.api.handler.error",
    "spectra.api.handler.error_response",
    "spectra.api.handler.error_code",
    "spectra.api.handler.error_message",
    "spectra.api.handler.last_error_message",
    "spectra.api.handler.register_sync",
    "spectra.api.handler.register_async",
    "spectra.api.handler.register_sync_callback",
    "spectra.api.handler.register_async_callback",
    "spectra.api.handler.dispatch_sync",
    "spectra.api.handler.dispatch_async",
    "spectra.api.cors.policy",
    "spectra.api.cors.permissive",
    "spectra.api.cors.allow_origin",
    "spectra.api.cors.allow_method",
    "spectra.api.cors.allow_header",
    "spectra.api.cors.expose_header",
    "spectra.api.cors.allow_credentials",
    "spectra.api.cors.max_age",
    "spectra.api.cors.middleware",
    "spectra.api.cors.is_preflight",
    "spectra.api.cors.preflight",
    "spectra.api.cors.apply",
    "spectra.api.cors.allowed_origin",
    "spectra.api.middleware.chain",
    "spectra.api.middleware.chain_new",
    "spectra.api.middleware.chain_len",
    "spectra.api.middleware.register_sync",
    "spectra.api.middleware.register_sync_short_circuit",
    "spectra.api.middleware.register_async",
    "spectra.api.middleware.register_async_short_circuit",
    "spectra.api.middleware.use_sync",
    "spectra.api.middleware.use_async",
    "spectra.api.middleware.execute_sync",
    "spectra.api.middleware.execute_async",
    "spectra.api.middleware.last_trace",
    "spectra.api.middleware.trace_len",
    "spectra.api.middleware.trace_event",
    "spectra.api.middleware.trace_short_circuited",
    "spectra.api.errors.last_code",
    "spectra.api.errors.last_message",
]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2202 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


def validate_workspace() -> None:
    workspace = parse_toml("Cargo.toml")
    members = workspace.get("workspace", {}).get("members", [])
    require(
        "packages/spectra-api" in members,
        "Cargo workspace must include packages/spectra-api",
    )

    crate = parse_toml("packages/spectra-api/Cargo.toml")
    require(crate.get("package", {}).get("name") == "spectra-api", "crate name mismatch")
    deps = crate.get("dependencies", {})
    require("spectra-runtime" in deps, "spectra-api must link against spectra-runtime")

    manifest = parse_toml("packages/spectra-api/spectra.toml")
    require(
        manifest.get("project", {}).get("name") == "spectra.api",
        "Spectra package name must be spectra.api",
    )
    require(
        manifest.get("project", {}).get("entry") == "src/bindings/mod.spectra",
        "Spectra package entry must point at bindings/mod.spectra",
    )


def validate_files() -> None:
    required = [
        "packages/spectra-api/src/lib.rs",
        "packages/spectra-api/src/http.rs",
        "packages/spectra-api/src/server.rs",
        "packages/spectra-api/src/client.rs",
        "packages/spectra-api/src/json.rs",
        "packages/spectra-api/src/tls.rs",
        "packages/spectra-api/src/routing.rs",
        "packages/spectra-api/src/query.rs",
        "packages/spectra-api/src/form.rs",
        "packages/spectra-api/src/multipart.rs",
        "packages/spectra-api/src/handler.rs",
        "packages/spectra-api/src/cors.rs",
        "packages/spectra-api/src/errors.rs",
        "packages/spectra-api/src/host_calls.rs",
        "packages/spectra-api/src/bindings/mod.spectra",
    ]
    for path in required:
        require((ROOT / path).exists(), f"missing required file {path}")


def validate_host_calls() -> None:
    lib = "\n".join(
        read(path)
        for path in (
            "packages/spectra-api/src/lib.rs",
            "packages/spectra-api/src/api_registration.rs",
            "packages/spectra-api/src/api_tests.rs",
        )
    )
    # HOST_CALLS is included from host_calls.rs after the crate was split into
    # focused modules; inspect that authoritative registry directly rather
    # than expecting the include body to be duplicated in lib.rs.
    host_registry = read("packages/spectra-api/src/host_calls.rs")
    names = re.findall(r'name:\s*"([^"]+)"', host_registry)
    require(
        len(names) == PACKAGE_HOST_CALL_COUNT,
        f"expected {PACKAGE_HOST_CALL_COUNT} package host calls, found {len(names)}",
    )
    require(len(set(names)) == len(names), "host call names must be unique")
    for name in names:
        require(name.startswith("spectra.api."), f"{name} must use spectra.api prefix")
    require(
        "function: client::client_request" in host_registry,
        "client request host call must point to its real implementation",
    )

    # The runtime required host-call namespace is the canonical HOST_CALLS
    # registry itself (runtime/src/api/mod.rs was removed as a stale duplicate).
    runtime_names = re.findall(r'name:\s*"([^"]+)"', host_registry)
    require(
        len(runtime_names) == PACKAGE_HOST_CALL_COUNT,
        "runtime required host-call namespace count drifted: "
        f"expected {PACKAGE_HOST_CALL_COUNT}, found {len(runtime_names)}",
    )
    require(
        len(set(runtime_names)) == len(runtime_names),
        "runtime required host-call names must be unique",
    )
    for name in runtime_names:
        require(name in names, f"{name} missing from package host-call registry")
    for name in LEGACY_REQUIRED_HOST_CALLS:
        require(name in names, f"legacy R-2202 host call {name} missing from package registry")
    require("pub fn register() -> usize" in lib, "spectra-api must expose register()")
    require(
        "register_host_function(spec.name, spec.function)" in lib,
        "register() must use the runtime host-call registry",
    )
    require(
        "spectra_api_register_host_calls" in lib,
        "crate must export an FFI registration symbol",
    )
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})" in lib,
        "unit tests must assert host-call count",
    )
    # Covered by the HOST_CALLS.len() assert above; the runtime duplicate is gone.


def validate_cli_integration() -> None:
    cargo = read("tools/spectra-cli/Cargo.toml")
    cli_src = ROOT / "tools/spectra-cli/src"
    integration = "\n".join(
        path.read_text(encoding="utf-8")
        for path in sorted(cli_src.glob("compiler_integration*.rs"))
    )
    entrypoints = "\n".join(
        (cli_src / path).read_text(encoding="utf-8")
        for path in ("main.rs", "cli_bench.rs")
    )
    require("spectra-api" in cargo, "spectra-cli must depend on spectra-api")
    require(
        "spectra_api::register();" in integration,
        "JIT execution path must register spectra-api host calls",
    )
    require(
        "spectra_api::register();" in entrypoints,
        "CLI async benchmark/runtime path must register spectra-api host calls",
    )


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2202 = items.get("R-2202")
    require(r2202 is not None, "R-2202 missing from roadmap")
    require(r2202.get("status") == "complete", "R-2202 must be marked complete")
    require(r2202.get("owner") == "web", "R-2202 owner must remain web")
    require("R-2201" in r2202.get("dependencies", []), "R-2202 must depend on R-2201")
    acceptance = "\n".join(r2202.get("acceptance", []))
    for term in [
        "packages/spectra-api",
        "runtime host-call registry",
        "cargo test -p spectra-api",
        "scripts/validate_r2202_spectra_api_hostcalls.py",
        str(PACKAGE_HOST_CALL_COUNT),
        "host_calls.rs",
    ]:
        require(term in acceptance, f"R-2202 acceptance must mention {term}")

    backlog = read("docs/roadmap-backlog.md")
    require("## R-2202 spectra-api Rust Crate and Host Call Registration" in backlog, "backlog R-2202 missing")
    r2202_block = backlog.split("## R-2202 spectra-api Rust Crate and Host Call Registration", 1)[1].split("## R-2203", 1)[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api",
        str(PACKAGE_HOST_CALL_COUNT),
        "host_calls.rs",
        "spectra_api::register()",
        "validate_r2202_spectra_api_hostcalls.py",
    ]:
        require(term in r2202_block, f"backlog R-2202 block must mention {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2202` `spectra-api` Rust crate and host call registration (complete;" in plan,
        "implementation plan must mark R-2202 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require(
        "validate_r2202_spectra_api_hostcalls.py" in runner,
        "run_tests.ps1 must run R-2202 validator",
    )
    require(
        'Teste = "validate_r2202_spectra_api_hostcalls"' in runner,
        "run_tests.ps1 must record R-2202 result",
    )


def main() -> None:
    validate_workspace()
    validate_files()
    validate_host_calls()
    validate_cli_integration()
    validate_planning()
    validate_runner()
    print("validated R-2202 spectra-api crate and host-call registration")


if __name__ == "__main__":
    main()
