use super::*;
use crate::semantic::module_registry::{ExportVisibility, ExportedFunction, ExportedSelfParamKind, ExportedTraitMethod, ExportedType, ModuleExports, ModuleRegistry};
use crate::ast::{Type, TypeAnnotation, TypeAnnotationKind};
use crate::span::Span;

/// Register all built-in standard library modules in the given registry.
pub fn register_builtin_modules(registry: &mut ModuleRegistry) {
    registry.register_module("std.io".to_string(), make_std_io());
    registry.register_module("std.math".to_string(), make_std_math());
    registry.register_module("std.numeric".to_string(), make_std_numeric());
    registry.register_module("std.collections".to_string(), make_std_collections());
    registry.register_module(
        "std.compat.collections".to_string(),
        make_std_compat_collections(),
    );
    registry.register_module("std.string".to_string(), make_std_string());
    registry.register_module("std.convert".to_string(), make_std_convert());
    registry.register_module("std.random".to_string(), make_std_random());
    registry.register_module("std.fs".to_string(), make_std_fs());
    registry.register_module("std.compat.fs".to_string(), make_std_compat_fs());
    registry.register_module("std.error".to_string(), make_std_error());
    registry.register_module("std.env".to_string(), make_std_env());
    registry.register_module("std.compat.env".to_string(), make_std_compat_env());
    registry.register_module("std.option".to_string(), make_std_option());
    registry.register_module("std.result".to_string(), make_std_result());
    registry.register_module("std.char".to_string(), make_std_char());
    registry.register_module("std.time".to_string(), make_std_time());
    registry.register_module("std.range".to_string(), make_std_range());
    registry.register_module("std.tensor".to_string(), make_std_tensor());
    registry.register_module("std.ml".to_string(), make_std_ml());
    registry.register_module("std.concurrent".to_string(), make_std_concurrent());
    registry.register_module("std.serve".to_string(), make_std_serve());
    register_std_api_modules(registry, "std.api");
    // Convenience aliases used in existing examples
    registry.register_module("spectra.std.io".to_string(), make_std_io());
    registry.register_module("spectra.std.math".to_string(), make_std_math());
    registry.register_module("spectra.std.numeric".to_string(), make_std_numeric());
    registry.register_module(
        "spectra.std.collections".to_string(),
        make_std_collections(),
    );
    registry.register_module(
        "spectra.std.compat.collections".to_string(),
        make_std_compat_collections(),
    );
    registry.register_module("spectra.std.string".to_string(), make_std_string());
    registry.register_module("spectra.std.convert".to_string(), make_std_convert());
    registry.register_module("spectra.std.random".to_string(), make_std_random());
    registry.register_module("spectra.std.fs".to_string(), make_std_fs());
    registry.register_module(
        "spectra.std.compat.fs".to_string(),
        make_std_compat_fs(),
    );
    registry.register_module("spectra.std.error".to_string(), make_std_error());
    registry.register_module("spectra.std.env".to_string(), make_std_env());
    registry.register_module(
        "spectra.std.compat.env".to_string(),
        make_std_compat_env(),
    );
    registry.register_module("spectra.std.option".to_string(), make_std_option());
    registry.register_module("spectra.std.result".to_string(), make_std_result());
    registry.register_module("spectra.std.char".to_string(), make_std_char());
    registry.register_module("spectra.std.time".to_string(), make_std_time());
    registry.register_module("spectra.std.range".to_string(), make_std_range());
    registry.register_module("spectra.std.tensor".to_string(), make_std_tensor());
    registry.register_module("spectra.std.ml".to_string(), make_std_ml());
    registry.register_module("spectra.std.concurrent".to_string(), make_std_concurrent());
    registry.register_module("spectra.std.serve".to_string(), make_std_serve());
    register_std_api_modules(registry, "spectra.std.api");
}

pub(crate) fn pub_fn(params: Vec<Type>, return_type: Type) -> ExportedFunction {
    ExportedFunction {
        params,
        return_type,
        visibility: ExportVisibility::Public,
        is_async: false,
    }
}

pub(crate) fn exported_trait_method(
    params: Vec<Type>,
    return_type: Type,
    self_kind: Option<ExportedSelfParamKind>,
    is_async: bool,
) -> ExportedTraitMethod {
    ExportedTraitMethod {
        params,
        return_type,
        self_kind,
        is_async,
        has_default: false,
    }
}

pub(crate) fn public_type(members: &[&str]) -> ExportedType {
    ExportedType {
        members: members.iter().map(|member| (*member).to_string()).collect(),
        visibility: ExportVisibility::Public,
        is_enum: false,
        struct_fields: None,
        enum_variants: None,
        enum_struct_variants: None,
    }
}

pub(crate) fn builtin_type_annotation(name: &str) -> TypeAnnotation {
    TypeAnnotation {
        kind: TypeAnnotationKind::Simple {
            segments: vec![name.to_string()],
        },
        span: Span::dummy(),
    }
}

pub(crate) fn api_type(name: &str) -> Type {
    Type::Struct {
        name: name.to_string(),
    }
}

pub(crate) fn api_task(output: Type) -> Type {
    Type::Task {
        output: Box::new(output),
    }
}

fn register_std_api_modules(registry: &mut ModuleRegistry, prefix: &str) {
    registry.register_module(prefix.to_string(), make_std_api_root(prefix));
    registry.register_module(format!("{prefix}.http"), make_std_api_http(prefix));
    registry.register_module(format!("{prefix}.http3"), make_std_api_http3(prefix));
    registry.register_module(format!("{prefix}.grpc"), make_std_api_grpc(prefix));
    registry.register_module(format!("{prefix}.graphql"), make_std_api_graphql(prefix));
    registry.register_module(format!("{prefix}.server"), make_std_api_server(prefix));
    registry.register_module(format!("{prefix}.client"), make_std_api_client(prefix));
    registry.register_module(format!("{prefix}.json"), make_std_api_json(prefix));
    registry.register_module(format!("{prefix}.jwt"), make_std_api_jwt(prefix));
    registry.register_module(format!("{prefix}.oauth"), make_std_api_oauth(prefix));
    registry.register_module(format!("{prefix}.tls"), make_std_api_tls(prefix));
    registry.register_module(format!("{prefix}.routing"), make_std_api_routing(prefix));
    registry.register_module(format!("{prefix}.query"), make_std_api_query(prefix));
    registry.register_module(format!("{prefix}.form"), make_std_api_form(prefix));
    registry.register_module(
        format!("{prefix}.multipart"),
        make_std_api_multipart(prefix),
    );
    registry.register_module(format!("{prefix}.handler"), make_std_api_handler(prefix));
    registry.register_module(format!("{prefix}.cors"), make_std_api_cors(prefix));
    registry.register_module(
        format!("{prefix}.middleware"),
        make_std_api_middleware(prefix),
    );
    registry.register_module(
        format!("{prefix}.validation"),
        make_std_api_validation(prefix),
    );
    registry.register_module(format!("{prefix}.errors"), make_std_api_errors(prefix));
    registry.register_module(
        format!("{prefix}.security"),
        make_std_api_security(prefix),
    );
    registry.register_module(format!("{prefix}.session"), make_std_api_session(prefix));
    registry.register_module(
        format!("{prefix}.websocket"),
        make_std_api_websocket(prefix),
    );
    registry.register_module(format!("{prefix}.sse"), make_std_api_sse(prefix));
    registry.register_module(format!("{prefix}.trace"), make_std_api_trace(prefix));
    registry.register_module(format!("{prefix}.health"), make_std_api_health(prefix));
    registry.register_module(format!("{prefix}.db.sqlite"), make_std_api_db_sqlite(prefix));
    registry.register_module(format!("{prefix}.db.postgres"), make_std_api_db_postgres(prefix));
    registry.register_module(format!("{prefix}.db.redis"), make_std_api_db_redis(prefix));
    registry.register_module(format!("{prefix}.db.pool"), make_std_api_db_pool(prefix));
    registry.register_module(
        format!("{prefix}.db.migrate"),
        make_std_api_db_migrate(prefix),
    );
}

fn stdlib_segments(prefix: &str) -> Vec<String> {
    prefix.split('.').map(|part| part.to_string()).collect()
}

pub(crate) fn api_module(prefix: &str, leaf: Option<&str>) -> ModuleExports {
    let mut stdlib_path = stdlib_segments(prefix);
    if let Some(leaf) = leaf {
        stdlib_path.push(leaf.to_string());
    }
    ModuleExports {
        stdlib_path: Some(stdlib_path),
        package_name: Some("spectra.api".to_string()),
        ..Default::default()
    }
}

fn make_std_api_root(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, None);
    for name in [
        "Request",
        "Response",
        "Router",
        "Server",
        "Client",
        "TlsConfig",
        "SqliteConnection",
        "SqliteStatement",
        "SessionStore",
        "Session",
        "WebSocketServer",
        "WebSocketClient",
        "WebSocket",
        "WebSocketMessage",
        "SseServer",
        "SseConnection",
        "SseEvent",
    ] {
        exports.types.insert(name.to_string(), public_type(&[]));
    }
    exports
}

fn make_std_range() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "range".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    exports.types.insert("Range".to_string(), public_type(&[]));

    let range = Type::Range;
    exports.functions.insert(
        "create".to_string(),
        pub_fn(vec![Type::Int, Type::Int, Type::Bool], range.clone()),
    );
    exports
        .functions
        .insert("len".to_string(), pub_fn(vec![range.clone()], Type::Int));
    exports.functions.insert(
        "at".to_string(),
        pub_fn(vec![range.clone(), Type::Int], Type::Int),
    );
    exports.functions.insert(
        "eq".to_string(),
        pub_fn(vec![range.clone(), range.clone()], Type::Bool),
    );
    exports
        .functions
        .insert("start".to_string(), pub_fn(vec![range.clone()], Type::Int));
    exports
        .functions
        .insert("end".to_string(), pub_fn(vec![range.clone()], Type::Int));
    exports
        .functions
        .insert("is_inclusive".to_string(), pub_fn(vec![range], Type::Bool));

    exports.functions.insert(
        "iter".to_string(),
        pub_fn(
            vec![Type::Range],
            Type::Struct {
                name: "Iterator".to_string(),
            },
        ),
    );

    exports
}

fn make_std_api_http(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("http"));
    for (name, members) in [
        ("Request", &["method", "path", "headers", "body"][..]),
        ("Response", &["status", "headers", "body"][..]),
        ("Method", &["code", "name"][..]),
        ("Status", &["code", "reason"][..]),
        ("Header", &["name", "value"][..]),
        ("Headers", &["len"][..]),
        (
            "Cookie",
            &[
                "name",
                "value",
                "path",
                "domain",
                "max_age",
                "secure",
                "http_only",
                "same_site",
            ][..],
        ),
        ("Body", &["len"][..]),
    ] {
        exports.types.insert(name.to_string(), public_type(members));
    }

    let request = api_type("Request");
    let response = api_type("Response");
    let header = api_type("Header");
    let cookie = api_type("Cookie");
    let status = api_type("Status");
    let functions = [
        ("method_name", vec![Type::Int], Type::String),
        ("method_allows_body", vec![Type::Int], Type::Bool),
        ("method_is_safe", vec![Type::Int], Type::Bool),
        ("method_get", vec![], Type::Int),
        ("method_head", vec![], Type::Int),
        ("method_post", vec![], Type::Int),
        ("method_put", vec![], Type::Int),
        ("method_patch", vec![], Type::Int),
        ("method_delete", vec![], Type::Int),
        ("method_options", vec![], Type::Int),
        ("status_reason", vec![Type::Int], Type::String),
        ("status_class", vec![Type::Int], Type::Int),
        ("status_is_success", vec![Type::Int], Type::Bool),
        ("status_continue", vec![], Type::Int),
        ("status_switching_protocols", vec![], Type::Int),
        ("status_ok", vec![], Type::Int),
        ("status_created", vec![], Type::Int),
        ("status_accepted", vec![], Type::Int),
        ("status_no_content", vec![], Type::Int),
        ("status_moved_permanently", vec![], Type::Int),
        ("status_found", vec![], Type::Int),
        ("status_not_modified", vec![], Type::Int),
        ("status_bad_request", vec![], Type::Int),
        ("status_unauthorized", vec![], Type::Int),
        ("status_forbidden", vec![], Type::Int),
        ("status_not_found", vec![], Type::Int),
        ("status_method_not_allowed", vec![], Type::Int),
        ("status_conflict", vec![], Type::Int),
        ("status_unsupported_media_type", vec![], Type::Int),
        ("status_unprocessable_content", vec![], Type::Int),
        ("status_too_many_requests", vec![], Type::Int),
        ("status_internal_server_error", vec![], Type::Int),
        ("status_bad_gateway", vec![], Type::Int),
        ("status_service_unavailable", vec![], Type::Int),
        ("status_gateway_timeout", vec![], Type::Int),
        ("header_name_is_valid", vec![Type::String], Type::Bool),
        ("header_value_is_valid", vec![Type::String], Type::Bool),
        ("request", vec![Type::Int, Type::String], request.clone()),
        ("request_new", vec![Type::Int], request.clone()),
        ("request_method", vec![request.clone()], Type::Int),
        ("request_path", vec![request.clone()], Type::String),
        ("request_body", vec![request.clone()], Type::String),
        (
            "request_header",
            vec![request.clone(), Type::String],
            Type::String,
        ),
        (
            "request_with_header",
            vec![request.clone(), Type::String, Type::String],
            request.clone(),
        ),
        (
            "request_with_body",
            vec![request.clone(), Type::String],
            request.clone(),
        ),
        (
            "request_cookie",
            vec![request.clone(), Type::String],
            Type::String,
        ),
        ("response", vec![Type::Int], response.clone()),
        ("response_new", vec![Type::Int], response.clone()),
        ("response_status", vec![response.clone()], Type::Int),
        (
            "response_header",
            vec![response.clone(), Type::String],
            Type::String,
        ),
        ("response_body_len", vec![response.clone()], Type::Int),
        ("header", vec![Type::String, Type::String], header.clone()),
        ("header_name", vec![header.clone()], Type::String),
        ("header_value", vec![header], Type::String),
        ("cookie", vec![Type::String, Type::String], cookie.clone()),
        ("cookie_name", vec![cookie.clone()], Type::String),
        ("cookie_value", vec![cookie.clone()], Type::String),
        (
            "cookie_with_options",
            vec![
                Type::String,
                Type::String,
                Type::String,
                Type::String,
                Type::Int,
                Type::Bool,
                Type::Bool,
                Type::Int,
            ],
            cookie.clone(),
        ),
        ("cookie_path", vec![cookie.clone()], Type::String),
        ("cookie_domain", vec![cookie.clone()], Type::String),
        ("cookie_max_age", vec![cookie.clone()], Type::Int),
        ("cookie_secure", vec![cookie.clone()], Type::Bool),
        ("cookie_http_only", vec![cookie.clone()], Type::Bool),
        ("cookie_same_site", vec![cookie.clone()], Type::Int),
        ("cookie_header", vec![cookie.clone()], Type::String),
        (
            "response_with_cookie",
            vec![response.clone(), cookie.clone()],
            response.clone(),
        ),
        (
            "cookie_sign",
            vec![cookie.clone(), Type::String],
            cookie.clone(),
        ),
        (
            "cookie_verify",
            vec![cookie.clone(), Type::String],
            Type::Bool,
        ),
        ("cookie_is_expired", vec![cookie.clone()], Type::Bool),
        ("cookie_error_code", vec![], Type::Int),
        ("cookie_error_message", vec![], Type::String),
        ("status", vec![Type::Int], status),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_server(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("server"));
    exports
        .types
        .insert("Server".to_string(), public_type(&["state"]));
    let server = api_type("Server");
    let router = api_type("Router");
    let functions = [
        ("new", vec![], server.clone()),
        ("listen", vec![server.clone(), Type::Int], Type::Bool),
        ("serve", vec![server.clone(), router], api_task(Type::Int)),
        ("state", vec![server.clone()], Type::Int),
        ("shutdown", vec![server.clone()], Type::Bool),
        ("local_port", vec![server.clone()], Type::Int),
        ("signal", vec![server.clone(), Type::Int], Type::Bool),
        ("stats", vec![server, Type::Int], Type::Int),
        (
            "set_max_body_bytes",
            vec![api_type("Server"), Type::Int],
            Type::Bool,
        ),
        (
            "set_read_timeout",
            vec![api_type("Server"), Type::Int],
            Type::Bool,
        ),
        (
            "set_idle_timeout",
            vec![api_type("Server"), Type::Int],
            Type::Bool,
        ),
        (
            "set_tls_certificate",
            vec![api_type("Server"), Type::String, Type::String],
            Type::Bool,
        ),
        ("tls_local_port", vec![api_type("Server")], Type::Int),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_client(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("client"));
    exports
        .types
        .insert("Client".to_string(), public_type(&["timeout_ms"]));
    let client = api_type("Client");
    let request = api_type("Request");
    let response = api_type("Response");
    let functions = [
        ("new", vec![], client.clone()),
        ("request", vec![client.clone(), request], api_task(response)),
        ("timeout_ms", vec![client], Type::Int),
        (
            "set_ssrf_policy",
            vec![api_type("Client"), api_type("SsrfPolicy")],
            Type::Bool,
        ),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_json(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("json"));
    exports
        .types
        .insert("JsonValue".to_string(), public_type(&["kind"]));
    let functions = [
        ("validate", vec![Type::String], Type::Bool),
        ("kind", vec![Type::String], Type::Int),
        // `parse` returns an opaque runtime handle (int); 0 signals a parse
        // error. Handles are freed explicitly with `value_free`.
        ("parse", vec![Type::String], Type::Int),
        ("value_kind", vec![Type::Int], Type::Int),
        ("value_len", vec![Type::Int], Type::Int),
        ("value_get", vec![Type::Int, Type::String], Type::Int),
        ("value_at", vec![Type::Int, Type::Int], Type::Int),
        ("value_text", vec![Type::Int], Type::String),
        ("value_number_bits", vec![Type::Int], Type::Int),
        ("value_bool", vec![Type::Int], Type::Int),
        ("value_free", vec![Type::Int], Type::Unit),
        ("stringify", vec![Type::Int], Type::String),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_jwt(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("jwt"));
    exports.functions.insert(
        "sign".to_string(),
        pub_fn(vec![Type::String, Type::String, Type::String], Type::String),
    );
    exports.functions.insert(
        "verify".to_string(),
        pub_fn(
            vec![
                Type::String,
                Type::String,
                Type::String,
                Type::String,
                Type::String,
                Type::Int,
            ],
            Type::Bool,
        ),
    );
    exports
}

fn make_std_api_oauth(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("oauth"));
    exports
        .types
        .insert("OAuthClient".to_string(), public_type(&["client_id"]));
    exports
        .types
        .insert("OAuthToken".to_string(), public_type(&["access_token", "token_type"]));
    let client = api_type("OAuthClient");
    let token = api_type("OAuthToken");
    let functions = [
        (
            "client_new",
            vec![
                Type::String,
                Type::String,
                Type::String,
                Type::String,
                Type::String,
                Type::String,
            ],
            client.clone(),
        ),
        (
            "client_set_revocation_url",
            vec![client.clone(), Type::String],
            Type::Bool,
        ),
        (
            "authorization_url",
            vec![client.clone(), Type::String],
            Type::String,
        ),
        (
            "exchange_code",
            vec![client.clone(), Type::String, Type::String],
            token.clone(),
        ),
        ("refresh", vec![client.clone(), token.clone()], token.clone()),
        ("revoke", vec![client.clone(), token.clone()], Type::Bool),
        (
            "token_access_token",
            vec![token.clone()],
            Type::String,
        ),
        (
            "token_refresh_token",
            vec![token.clone()],
            Type::String,
        ),
        ("token_type", vec![token.clone()], Type::String),
        ("token_expires_at_ms", vec![token.clone()], Type::Int),
        ("token_scope", vec![token], Type::String),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_tls(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("tls"));
    exports
        .types
        .insert("TlsConfig".to_string(), public_type(&["mode"]));
    let tls_config = api_type("TlsConfig");
    let functions = [
        ("config_new", vec![Type::Int], tls_config.clone()),
        ("config_mode", vec![tls_config.clone()], Type::Int),
        ("client_config", vec![], tls_config),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_routing(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("routing"));
    exports
        .types
        .insert("Route".to_string(), public_type(&["method", "path"]));
    exports
        .types
        .insert("Router".to_string(), public_type(&["route_count"]));
    exports
        .types
        .insert("RouteMatch".to_string(), public_type(&["route_id"]));
    let router = api_type("Router");
    let route = api_type("Route");
    let route_match = api_type("RouteMatch");
    let mut functions = vec![
        ("router", vec![], router.clone()),
        ("router_new", vec![], router.clone()),
        ("route_count", vec![router.clone()], Type::Int),
        ("route_id", vec![route.clone()], Type::Int),
        (
            "route_add",
            vec![router.clone(), Type::Int, Type::String],
            route.clone(),
        ),
        (
            "route_match",
            vec![router.clone(), Type::Int, Type::String],
            route_match.clone(),
        ),
        ("match_route_id", vec![route_match.clone()], Type::Int),
        (
            "match_param",
            vec![route_match.clone(), Type::String],
            Type::String,
        ),
        (
            "match_param_int",
            vec![route_match, Type::String],
            Type::Int,
        ),
        ("last_conflict", vec![], Type::String),
        (
            "routes_export_openapi",
            vec![router.clone(), Type::String, Type::String],
            Type::String,
        ),
        (
            "routes_set_request_schema",
            vec![router.clone(), Type::Int, Type::String, Type::String],
            Type::Bool,
        ),
    ];
    for name in ["get", "post", "put", "patch", "delete"] {
        functions.push((name, vec![router.clone(), Type::String], route.clone()));
    }
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_query(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("query"));
    exports
        .types
        .insert("Query".to_string(), public_type(&["len"]));
    exports
        .types
        .insert("QuerySchema".to_string(), public_type(&["field_count"]));
    exports
        .types
        .insert("QueryBinding".to_string(), public_type(&["ok"]));
    let query = api_type("Query");
    let schema = api_type("QuerySchema");
    let binding = api_type("QueryBinding");
    let functions = [
        ("type_string", vec![], Type::Int),
        ("type_int", vec![], Type::Int),
        ("type_bool", vec![], Type::Int),
        ("parse", vec![Type::String], query.clone()),
        ("len", vec![query.clone()], Type::Int),
        ("has", vec![query.clone(), Type::String], Type::Bool),
        ("count", vec![query.clone(), Type::String], Type::Int),
        ("first", vec![query.clone(), Type::String], Type::String),
        (
            "value",
            vec![query.clone(), Type::String, Type::Int],
            Type::String,
        ),
        (
            "int",
            vec![query.clone(), Type::String, Type::Int],
            Type::Int,
        ),
        (
            "bool",
            vec![query.clone(), Type::String, Type::Int],
            Type::Bool,
        ),
        ("schema", vec![], schema.clone()),
        (
            "schema_field",
            vec![
                schema.clone(),
                Type::String,
                Type::Int,
                Type::Bool,
                Type::Bool,
            ],
            schema.clone(),
        ),
        ("bind", vec![query, schema], binding.clone()),
        ("binding_ok", vec![binding.clone()], Type::Bool),
        ("binding_error", vec![binding.clone()], Type::String),
        (
            "binding_count",
            vec![binding.clone(), Type::String],
            Type::Int,
        ),
        (
            "binding_value",
            vec![binding.clone(), Type::String, Type::Int],
            Type::String,
        ),
        (
            "binding_int",
            vec![binding.clone(), Type::String, Type::Int],
            Type::Int,
        ),
        (
            "binding_bool",
            vec![binding, Type::String, Type::Int],
            Type::Bool,
        ),
        ("error_code", vec![], Type::Int),
        ("error_message", vec![], Type::String),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_form(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("form"));
    exports
        .types
        .insert("Form".to_string(), public_type(&["len"]));
    exports
        .types
        .insert("FormSchema".to_string(), public_type(&["field_count"]));
    exports
        .types
        .insert("FormBinding".to_string(), public_type(&["ok"]));
    let form = api_type("Form");
    let schema = api_type("FormSchema");
    let binding = api_type("FormBinding");
    let functions = [
        ("type_string", vec![], Type::Int),
        ("type_int", vec![], Type::Int),
        ("type_bool", vec![], Type::Int),
        ("parse", vec![Type::String], form.clone()),
        ("len", vec![form.clone()], Type::Int),
        ("has", vec![form.clone(), Type::String], Type::Bool),
        ("count", vec![form.clone(), Type::String], Type::Int),
        ("first", vec![form.clone(), Type::String], Type::String),
        (
            "value",
            vec![form.clone(), Type::String, Type::Int],
            Type::String,
        ),
        (
            "int",
            vec![form.clone(), Type::String, Type::Int],
            Type::Int,
        ),
        (
            "bool",
            vec![form.clone(), Type::String, Type::Int],
            Type::Bool,
        ),
        ("schema", vec![], schema.clone()),
        (
            "schema_field",
            vec![
                schema.clone(),
                Type::String,
                Type::Int,
                Type::Bool,
                Type::Bool,
            ],
            schema.clone(),
        ),
        ("bind", vec![form, schema], binding.clone()),
        ("binding_ok", vec![binding.clone()], Type::Bool),
        ("binding_error", vec![binding.clone()], Type::String),
        (
            "binding_count",
            vec![binding.clone(), Type::String],
            Type::Int,
        ),
        (
            "binding_value",
            vec![binding.clone(), Type::String, Type::Int],
            Type::String,
        ),
        (
            "binding_int",
            vec![binding.clone(), Type::String, Type::Int],
            Type::Int,
        ),
        (
            "binding_bool",
            vec![binding, Type::String, Type::Int],
            Type::Bool,
        ),
        ("error_code", vec![], Type::Int),
        ("error_message", vec![], Type::String),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_multipart(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("multipart"));
    exports
        .types
        .insert("Multipart".to_string(), public_type(&["part_count"]));
    exports.types.insert(
        "MultipartPart".to_string(),
        public_type(&["name", "filename", "content_type", "size"]),
    );
    let multipart = api_type("Multipart");
    let part = api_type("MultipartPart");
    let functions = [
        (
            "parse",
            vec![Type::String, Type::String, Type::Int, Type::Int, Type::Int],
            multipart.clone(),
        ),
        ("part_count", vec![multipart.clone()], Type::Int),
        ("field_count", vec![multipart.clone()], Type::Int),
        ("file_count", vec![multipart.clone()], Type::Int),
        (
            "text",
            vec![multipart.clone(), Type::String, Type::Int],
            Type::String,
        ),
        ("part", vec![multipart, Type::Int], part.clone()),
        ("part_name", vec![part.clone()], Type::String),
        ("part_filename", vec![part.clone()], Type::String),
        ("part_content_type", vec![part.clone()], Type::String),
        ("part_size", vec![part.clone()], Type::Int),
        ("part_is_file", vec![part.clone()], Type::Bool),
        ("file_path", vec![part.clone()], Type::String),
        (
            "file_read",
            vec![part.clone(), Type::Int, Type::Int],
            Type::String,
        ),
        ("file_spool_to", vec![part, Type::String], Type::Bool),
        ("error_code", vec![], Type::Int),
        ("error_message", vec![], Type::String),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}
fn make_std_api_db_pool(prefix: &str) -> ModuleExports {
    let mut exports = api_module(&format!("{prefix}.db.pool"), None);
    let pool = api_type("Pool");
    let connection = api_type("SqliteConnection");
    exports.types.insert("Pool".to_string(), public_type(&[]));
    let functions = [
        (
            "sqlite_open",
            vec![Type::String, Type::Int],
            pool.clone(),
        ),
        ("close", vec![pool.clone()], Type::Bool),
        ("with_connection", vec![pool], connection),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_db_migrate(prefix: &str) -> ModuleExports {
    let mut exports = api_module(&format!("{prefix}.db.migrate"), None);
    let connection = api_type("SqliteConnection");
    let functions = [
        ("apply_sqlite", vec![connection.clone(), Type::String], Type::Int),
        (
            "status_sqlite",
            vec![connection, Type::String],
            Type::String,
        ),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}
fn make_std_api_http3(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("http3"));
    let functions = [
        ("server_start", vec![Type::Int, Type::Int], Type::Int),
        ("server_local_port", vec![Type::Int], Type::Int),
        ("server_shutdown", vec![Type::Int], api_task(Type::Int)),
        ("client_connect", vec![Type::Int, Type::String], api_task(Type::Int)),
        ("client_shutdown", vec![Type::Int], api_task(Type::Int)),
        (
            "client_request_new",
            vec![Type::Int, Type::String, Type::String],
            Type::Int,
        ),
        (
            "client_request_header",
            vec![Type::Int, Type::String, Type::String],
            Type::Bool,
        ),
        ("client_request_open", vec![Type::Int], api_task(Type::Int)),
        (
            "client_request_send_body",
            vec![Type::Int, Type::String],
            api_task(Type::Int),
        ),
        (
            "client_request_send_trailers",
            vec![Type::Int, Type::String, Type::String],
            api_task(Type::Int),
        ),
        ("client_request_finish", vec![Type::Int], api_task(Type::Int)),
        (
            "client_request_receive_response",
            vec![Type::Int],
            api_task(Type::Int),
        ),
        ("client_request_cancel", vec![Type::Int], Type::Bool),
        ("response_status", vec![Type::Int], Type::Int),
        ("response_header", vec![Type::Int, Type::String], Type::String),
        ("response_trailer", vec![Type::Int, Type::String], Type::String),
        ("response_body_base64", vec![Type::Int], Type::String),
        ("response_body_len", vec![Type::Int], Type::Int),
        ("task_result", vec![Type::Int], Type::Int),
        ("task_cancel", vec![Type::Int], Type::Bool),
        ("result_ok", vec![Type::Int], Type::Bool),
        ("result_value", vec![Type::Int], Type::Int),
        ("result_error_code", vec![Type::Int], Type::Int),
        ("result_error_message", vec![Type::Int], Type::String),
        ("handle_drop", vec![Type::Int], Type::Bool),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_grpc(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("grpc"));
    let functions = [
        ("message_from_base64", vec![Type::String], Type::Int),
        ("message_to_base64", vec![Type::Int], Type::String),
        ("message_len", vec![Type::Int], Type::Int),
        ("message_free", vec![Type::Int], Type::Bool),
        ("metadata_new", vec![], Type::Int),
        (
            "metadata_insert",
            vec![Type::Int, Type::String, Type::String],
            Type::Bool,
        ),
        (
            "metadata_append",
            vec![Type::Int, Type::String, Type::String],
            Type::Bool,
        ),
        ("metadata_get", vec![Type::Int, Type::String], Type::String),
        ("metadata_len", vec![Type::Int], Type::Int),
        ("metadata_free", vec![Type::Int], Type::Bool),
        ("status_new", vec![Type::Int, Type::String], Type::Int),
        ("status_code", vec![Type::Int], Type::Int),
        ("status_message", vec![Type::Int], Type::String),
        ("status_details_base64", vec![Type::Int], Type::String),
        (
            "status_set_details_base64",
            vec![Type::Int, Type::String],
            Type::Bool,
        ),
        ("status_free", vec![Type::Int], Type::Bool),
        ("response_message", vec![Type::Int], Type::Int),
        ("response_status", vec![Type::Int], Type::Int),
        ("response_metadata", vec![Type::Int], Type::Int),
        ("response_free", vec![Type::Int], Type::Bool),
        ("error_code", vec![Type::Int], Type::Int),
        ("error_message", vec![Type::Int], Type::String),
        ("error_details_base64", vec![Type::Int], Type::String),
        ("error_free", vec![Type::Int], Type::Bool),
        ("client_connect", vec![Type::String], api_task(Type::Int)),
        (
            "client_unary",
            vec![Type::Int, Type::String, Type::Int, Type::Int, Type::Int],
            api_task(Type::Int),
        ),
        (
            "client_client_streaming",
            vec![Type::Int, Type::String, Type::Int, Type::Int, Type::Int],
            api_task(Type::Int),
        ),
        (
            "client_server_streaming",
            vec![Type::Int, Type::String, Type::Int, Type::Int, Type::Int],
            api_task(Type::Int),
        ),
        (
            "client_bidi_streaming",
            vec![Type::Int, Type::String, Type::Int, Type::Int, Type::Int],
            api_task(Type::Int),
        ),
        ("stream_send", vec![Type::Int, Type::Int], api_task(Type::Int)),
        ("stream_recv", vec![Type::Int], api_task(Type::Int)),
        ("stream_finish", vec![Type::Int], api_task(Type::Int)),
        ("stream_cancel", vec![Type::Int], api_task(Type::Int)),
        ("stream_free", vec![Type::Int], Type::Bool),
        (
            "server_bind",
            vec![Type::String, Type::Int, Type::Int, Type::Int, Type::Int],
            api_task(Type::Int),
        ),
        ("server_local_port", vec![Type::Int], Type::Int),
        ("server_shutdown", vec![Type::Int], Type::Bool),
        ("server_free", vec![Type::Int], Type::Bool),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_graphql(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("graphql"));
    let functions = [
        ("schema_new", vec![], Type::Int),
        ("schema_set_workers", vec![Type::Int, Type::Int], Type::Bool),
        (
            "schema_set_subscription_capacity",
            vec![Type::Int, Type::Int],
            Type::Bool,
        ),
        (
            "schema_field_json",
            vec![Type::Int, Type::Int, Type::String, Type::String, Type::String],
            Type::Bool,
        ),
        (
            "schema_field_callback",
            vec![Type::Int, Type::Int, Type::String, Type::String, Type::Int],
            Type::Bool,
        ),
        (
            "schema_subscription_json",
            vec![Type::Int, Type::String, Type::String, Type::String],
            Type::Bool,
        ),
        (
            "schema_subscription_callback",
            vec![Type::Int, Type::String, Type::String, Type::Int],
            Type::Bool,
        ),
        ("schema_finish", vec![Type::Int], Type::Int),
        ("schema_drop", vec![Type::Int], Type::Bool),
        ("schema_sdl", vec![Type::Int], Type::String),
        ("execute", vec![Type::Int, Type::String, Type::String], Type::Int),
        (
            "execute_named",
            vec![Type::Int, Type::String, Type::String, Type::String],
            Type::Int,
        ),
        (
            "execute_http",
            vec![Type::Int, Type::Int, Type::String, Type::String, Type::String],
            Type::Int,
        ),
        ("response_json", vec![Type::Int], Type::String),
        ("response_status", vec![Type::Int], Type::Int),
        ("response_is_ok", vec![Type::Int], Type::Bool),
        ("response_errors_json", vec![Type::Int], Type::String),
        ("response_data_json", vec![Type::Int], Type::String),
        ("response_drop", vec![Type::Int], Type::Bool),
        ("subscribe", vec![Type::Int, Type::String, Type::String], Type::Int),
        ("subscription_next", vec![Type::Int], Type::Int),
        ("subscription_pending", vec![Type::Int], Type::Int),
        ("subscription_capacity", vec![Type::Int], Type::Int),
        ("subscription_is_cancelled", vec![Type::Int], Type::Bool),
        ("subscription_cancel", vec![Type::Int], Type::Bool),
        ("subscription_drop", vec![Type::Int], Type::Bool),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}
