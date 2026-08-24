// Builtin (virtual) module registrations
// Maps well-known `std.*` module paths to their exported function signatures
// without requiring physical `.spectra` files.  The actual implementation of
// each function lives in the runtime FFI layer (runtime/src/stdlib/mod.rs).

use super::module_registry::{
    ExportVisibility, ExportedFunction, ExportedSelfParamKind, ExportedTrait, ExportedTraitMethod,
    ExportedType, ModuleExports, ModuleRegistry,
};
use crate::ast::{FloatWidth, IntWidth, Type, TypeAnnotation, TypeAnnotationKind};
use crate::span::Span;
use std::collections::HashMap;

/// Compiler-owned snapshot of one public builtin contract symbol.
///
/// Tooling can serialize this plain data without depending on the runtime
/// crate or reconstructing semantic declarations from source-text regexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinContractSymbol {
    pub path: String,
    pub kind: &'static str,
    pub signature: String,
}

fn contract_type(ty: &Type) -> String {
    match ty {
        Type::Int => "int".to_string(),
        Type::Float => "float".to_string(),
        Type::ExactInt { signed, width } => {
            let name = match width {
                IntWidth::I8 => "8",
                IntWidth::I16 => "16",
                IntWidth::I32 => "32",
                IntWidth::I64 => "64",
                IntWidth::Isize | IntWidth::Usize => "size",
            };
            if *signed {
                format!("i{name}")
            } else {
                format!("u{name}")
            }
        }
        Type::ExactFloat { width } => match width {
            FloatWidth::F32 => "f32".to_string(),
            FloatWidth::F64 => "f64".to_string(),
        },
        Type::Bool => "bool".to_string(),
        Type::String => "string".to_string(),
        Type::Char => "char".to_string(),
        Type::Unit => "unit".to_string(),
        Type::Unknown => "unknown".to_string(),
        Type::Array { element_type, size } => match size {
            Some(size) => format!("Array<{}, {size}>", contract_type(element_type)),
            None => format!("Array<{}>", contract_type(element_type)),
        },
        Type::Tuple { elements } => format!(
            "({})",
            elements.iter().map(contract_type).collect::<Vec<_>>().join(", ")
        ),
        Type::Struct { name } | Type::Enum { name } | Type::TypeParameter { name } => name.clone(),
        Type::Applied { name, args } => format!(
            "{}<{}>",
            name,
            args.iter().map(contract_type).collect::<Vec<_>>().join(", ")
        ),
        Type::SelfType => "Self".to_string(),
        Type::Fn { params, return_type } => format!(
            "fn({}) -> {}",
            params.iter().map(contract_type).collect::<Vec<_>>().join(", "),
            contract_type(return_type)
        ),
        Type::Task { output } => format!("Task<{}>", contract_type(output)),
        Type::Range => "Range".to_string(),
        Type::Tensor {
            dtype,
            rank,
            dims,
            layout,
            device,
        } => {
            let mut metadata = vec![contract_type(dtype)];
            if let Some(rank) = rank {
                metadata.push(format!("rank={rank}"));
            }
            if let Some(dims) = dims {
                metadata.push(format!(
                    "dims={:?}",
                    dims.iter()
                        .map(|dim| dim.map_or("?".to_string(), |value| value.to_string()))
                        .collect::<Vec<_>>()
                ));
            }
            if let Some(layout) = layout {
                metadata.push(format!("layout={layout}"));
            }
            if let Some(device) = device {
                metadata.push(format!("device={device}"));
            }
            format!("Tensor<{}>", metadata.join(", "))
        }
        Type::DynTrait {
            trait_name,
            auto_traits,
        } => {
            if auto_traits.is_empty() {
                format!("dyn {trait_name}")
            } else {
                format!("dyn {trait_name} + {}", auto_traits.join(" + "))
            }
        }
    }
}

fn contract_annotation(annotation: &TypeAnnotation) -> String {
    match &annotation.kind {
        TypeAnnotationKind::Simple { segments } => segments.join("::"),
        TypeAnnotationKind::Tuple { elements } => format!(
            "({})",
            elements
                .iter()
                .map(contract_annotation)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeAnnotationKind::Function {
            params,
            return_type,
        } => format!(
            "fn({}) -> {}",
            params
                .iter()
                .map(contract_annotation)
                .collect::<Vec<_>>()
                .join(", "),
            contract_annotation(return_type)
        ),
        TypeAnnotationKind::Generic { name, type_args } => format!(
            "{}<{}>",
            name,
            type_args
                .iter()
                .map(contract_annotation)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeAnnotationKind::DynTrait {
            trait_name,
            auto_traits,
        } => {
            if auto_traits.is_empty() {
                format!("dyn {trait_name}")
            } else {
                format!("dyn {trait_name} + {}", auto_traits.join(" + "))
            }
        }
    }
}

fn exported_type_signature(name: &str, exported: &ExportedType) -> String {
    if exported.is_enum {
        let mut variants = exported
            .enum_variants
            .as_ref()
            .map(|variants| variants.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        variants.sort();
        format!("enum {name} {{ {} }}", variants.join(" | "))
    } else {
        let mut fields = exported
            .struct_fields
            .as_ref()
            .map(|fields| {
                fields
                    .iter()
                    .map(|(field, ty)| format!("{field}: {}", contract_annotation(ty)))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        fields.sort();
        format!("record {name} {{ {} }}", fields.join(", "))
    }
}

/// Extract the builtin semantic surface used by contract tooling.
pub fn builtin_contract_symbols() -> Vec<BuiltinContractSymbol> {
    let mut registry = ModuleRegistry::new();
    register_builtin_modules(&mut registry);
    let mut symbols = Vec::new();

    for (module_path, exports) in registry.iter_modules() {
        symbols.push(BuiltinContractSymbol {
            path: module_path.to_string(),
            kind: "module",
            signature: format!("module {module_path}"),
        });
        for (name, exported) in &exports.types {
            symbols.push(BuiltinContractSymbol {
                path: format!("{module_path}.{name}"),
                kind: "type",
                signature: exported_type_signature(name, exported),
            });
        }
        for (name, exported) in &exports.functions {
            symbols.push(BuiltinContractSymbol {
                path: format!("{module_path}.{name}"),
                kind: "function",
                signature: format!(
                    "fn({}) -> {}",
                    exported
                        .params
                        .iter()
                        .map(contract_type)
                        .collect::<Vec<_>>()
                        .join(", "),
                    contract_type(&exported.return_type)
                ),
            });
        }
    }
    symbols.sort_by(|left, right| left.path.cmp(&right.path));
    symbols
}

pub const STD_API_MODULE_PATHS: &[&str] = &[
    "std.api",
    "std.api.http",
    "std.api.server",
    "std.api.client",
    "std.api.json",
    "std.api.jwt",
    "std.api.oauth",
    "std.api.tls",
    "std.api.routing",
    "std.api.query",
    "std.api.form",
    "std.api.multipart",
    "std.api.handler",
    "std.api.cors",
    "std.api.middleware",
    "std.api.validation",
    "std.api.errors",
    "std.api.security",
    "std.api.session",
    "std.api.websocket",
    "std.api.sse",
];

pub const STD_API_PUBLIC_TYPES: &[(&str, &str)] = &[
    ("std.api.http.Request", "record Request"),
    ("std.api.http.Response", "record Response"),
    ("std.api.http.Method", "record Method"),
    ("std.api.http.Status", "record Status"),
    ("std.api.http.Header", "record Header"),
    ("std.api.http.Headers", "record Headers"),
    ("std.api.http.Cookie", "record Cookie"),
    ("std.api.http.Body", "record Body"),
    ("std.api.server.Server", "record Server"),
    ("std.api.client.Client", "record Client"),
    ("std.api.json.JsonValue", "record JsonValue"),
    ("std.api.oauth.OAuthClient", "record OAuthClient"),
    ("std.api.oauth.OAuthToken", "record OAuthToken"),
    ("std.api.tls.TlsConfig", "record TlsConfig"),
    ("std.api.routing.Route", "record Route"),
    ("std.api.routing.Router", "record Router"),
    ("std.api.routing.RouteMatch", "record RouteMatch"),
    ("std.api.query.Query", "record Query"),
    ("std.api.query.QuerySchema", "record QuerySchema"),
    ("std.api.query.QueryBinding", "record QueryBinding"),
    ("std.api.form.Form", "record Form"),
    ("std.api.form.FormSchema", "record FormSchema"),
    ("std.api.form.FormBinding", "record FormBinding"),
    ("std.api.multipart.Multipart", "record Multipart"),
    ("std.api.multipart.MultipartPart", "record MultipartPart"),
    ("std.api.handler.HandlerHandle", "record HandlerHandle"),
    (
        "std.api.handler.AsyncHandlerHandle",
        "record AsyncHandlerHandle",
    ),
    ("std.api.handler.HandlerError", "record HandlerError"),
    (
        "std.api.middleware.MiddlewareChain",
        "record MiddlewareChain",
    ),
    (
        "std.api.middleware.MiddlewareHandle",
        "record MiddlewareHandle",
    ),
    (
        "std.api.middleware.AsyncMiddlewareHandle",
        "record AsyncMiddlewareHandle",
    ),
    (
        "std.api.middleware.MiddlewareTrace",
        "record MiddlewareTrace",
    ),
    (
        "std.api.validation.ValidationSchema",
        "record ValidationSchema",
    ),
    (
        "std.api.validation.ValidationResult",
        "record ValidationResult",
    ),
    ("std.api.cors.CorsPolicy", "record CorsPolicy"),
    ("std.api.errors.ApiError", "record ApiError"),
    ("std.api.security.CsrfPolicy", "record CsrfPolicy"),
    ("std.api.security.SsrfPolicy", "record SsrfPolicy"),
    ("std.api.session.SessionStore", "record SessionStore"),
    ("std.api.session.Session", "record Session"),
    (
        "std.api.websocket.WebSocketServer",
        "record WebSocketServer",
    ),
    (
        "std.api.websocket.WebSocketClient",
        "record WebSocketClient",
    ),
    ("std.api.websocket.WebSocket", "record WebSocket"),
    (
        "std.api.websocket.WebSocketMessage",
        "record WebSocketMessage",
    ),
    ("std.api.sse.SseServer", "record SseServer"),
    ("std.api.sse.SseConnection", "record SseConnection"),
    ("std.api.sse.SseEvent", "record SseEvent"),
];

pub const STD_API_PUBLIC_FUNCTIONS: &[(&str, &str)] = &[
    ("std.api.http.method_name", "func(int) returns string"),
    ("std.api.http.method_allows_body", "func(int) returns bool"),
    ("std.api.http.method_is_safe", "func(int) returns bool"),
    ("std.api.http.method_get", "func() returns int"),
    ("std.api.http.method_head", "func() returns int"),
    ("std.api.http.method_post", "func() returns int"),
    ("std.api.http.method_put", "func() returns int"),
    ("std.api.http.method_patch", "func() returns int"),
    ("std.api.http.method_delete", "func() returns int"),
    ("std.api.http.method_options", "func() returns int"),
    ("std.api.http.status_reason", "func(int) returns string"),
    ("std.api.http.status_class", "func(int) returns int"),
    ("std.api.http.status_is_success", "func(int) returns bool"),
    ("std.api.http.status_continue", "func() returns int"),
    ("std.api.http.status_switching_protocols", "func() returns int"),
    ("std.api.http.status_ok", "func() returns int"),
    ("std.api.http.status_created", "func() returns int"),
    ("std.api.http.status_accepted", "func() returns int"),
    ("std.api.http.status_no_content", "func() returns int"),
    ("std.api.http.status_moved_permanently", "func() returns int"),
    ("std.api.http.status_found", "func() returns int"),
    ("std.api.http.status_not_modified", "func() returns int"),
    ("std.api.http.status_bad_request", "func() returns int"),
    ("std.api.http.status_unauthorized", "func() returns int"),
    ("std.api.http.status_forbidden", "func() returns int"),
    ("std.api.http.status_not_found", "func() returns int"),
    ("std.api.http.status_method_not_allowed", "func() returns int"),
    ("std.api.http.status_conflict", "func() returns int"),
    ("std.api.http.status_unsupported_media_type", "func() returns int"),
    ("std.api.http.status_unprocessable_content", "func() returns int"),
    ("std.api.http.status_too_many_requests", "func() returns int"),
    ("std.api.http.status_internal_server_error", "func() returns int"),
    ("std.api.http.status_bad_gateway", "func() returns int"),
    ("std.api.http.status_service_unavailable", "func() returns int"),
    ("std.api.http.status_gateway_timeout", "func() returns int"),
    ("std.api.http.header_name_is_valid", "func(string) returns bool"),
    ("std.api.http.header_value_is_valid", "func(string) returns bool"),
    ("std.api.http.request", "func(int, string) returns Request"),
    ("std.api.http.request_new", "func(int) returns Request"),
    ("std.api.http.request_method", "func(Request) returns int"),
    ("std.api.http.request_path", "func(Request) returns string"),
    ("std.api.http.request_body", "func(Request) returns string"),
    (
        "std.api.http.request_header",
        "func(Request, string) returns string",
    ),
    (
        "std.api.http.request_with_header",
        "func(Request, string, string) returns Request",
    ),
    (
        "std.api.http.request_with_body",
        "func(Request, string) returns Request",
    ),
    (
        "std.api.http.request_cookie",
        "func(Request, string) returns string",
    ),
    ("std.api.http.response", "func(int) returns Response"),
    ("std.api.http.response_new", "func(int) returns Response"),
    ("std.api.http.response_status", "func(Response) returns int"),
    (
        "std.api.http.response_header",
        "func(Response, string) returns string",
    ),
    ("std.api.http.response_body_len", "func(Response) returns int"),
    ("std.api.http.header", "func(string, string) returns Header"),
    ("std.api.http.header_name", "func(Header) returns string"),
    ("std.api.http.header_value", "func(Header) returns string"),
    ("std.api.http.cookie", "func(string, string) returns Cookie"),
    ("std.api.http.cookie_name", "func(Cookie) returns string"),
    ("std.api.http.cookie_value", "func(Cookie) returns string"),
    (
        "std.api.http.cookie_with_options",
        "func(string, string, string, string, int, bool, bool, int) returns Cookie",
    ),
    ("std.api.http.cookie_path", "func(Cookie) returns string"),
    ("std.api.http.cookie_domain", "func(Cookie) returns string"),
    ("std.api.http.cookie_max_age", "func(Cookie) returns int"),
    ("std.api.http.cookie_secure", "func(Cookie) returns bool"),
    ("std.api.http.cookie_http_only", "func(Cookie) returns bool"),
    ("std.api.http.cookie_same_site", "func(Cookie) returns int"),
    ("std.api.http.cookie_header", "func(Cookie) returns string"),
    (
        "std.api.http.response_with_cookie",
        "func(Response, Cookie) returns Response",
    ),
    (
        "std.api.http.cookie_sign",
        "func(Cookie, string) returns Cookie",
    ),
    (
        "std.api.http.cookie_verify",
        "func(Cookie, string) returns bool",
    ),
    ("std.api.http.cookie_is_expired", "func(Cookie) returns bool"),
    ("std.api.http.cookie_error_code", "func() returns int"),
    ("std.api.http.cookie_error_message", "func() returns string"),
    ("std.api.http.status", "func(int) returns Status"),
    ("std.api.server.new", "func() returns Server"),
    ("std.api.server.listen", "func(Server, int) returns bool"),
    ("std.api.server.serve", "func(Server, Router) returns task<int>"),
    ("std.api.server.state", "func(Server) returns int"),
    ("std.api.server.shutdown", "func(Server) returns bool"),
    ("std.api.server.local_port", "func(Server) returns int"),
    ("std.api.server.signal", "func(Server, int) returns bool"),
    ("std.api.server.stats", "func(Server, int) returns int"),
    (
        "std.api.server.set_max_body_bytes",
        "func(Server, int) returns bool",
    ),
    (
        "std.api.server.set_read_timeout",
        "func(Server, int) returns bool",
    ),
    (
        "std.api.server.set_idle_timeout",
        "func(Server, int) returns bool",
    ),
    ("std.api.client.new", "func() returns Client"),
    (
        "std.api.client.request",
        "func(Client, Request) returns task<Response>",
    ),
    ("std.api.client.timeout_ms", "func(Client) returns int"),
    (
        "std.api.client.set_ssrf_policy",
        "func(Client, SsrfPolicy) returns bool",
    ),
    ("std.api.json.validate", "func(string) returns bool"),
    ("std.api.json.kind", "func(string) returns int"),
    ("std.api.json.encode", "func(unknown) returns string"),
    ("std.api.json.decode", "func(string) returns JsonValue"),
    (
        "std.api.jwt.sign",
        "func(string, string, string) returns string",
    ),
    (
        "std.api.jwt.verify",
        "func(string, string, string, string, string, int) returns bool",
    ),
    (
        "std.api.oauth.client_new",
        "func(string, string, string, string, string, string) returns OAuthClient",
    ),
    (
        "std.api.oauth.client_set_revocation_url",
        "func(OAuthClient, string) returns bool",
    ),
    (
        "std.api.oauth.authorization_url",
        "func(OAuthClient, string) returns string",
    ),
    (
        "std.api.oauth.exchange_code",
        "func(OAuthClient, string, string) returns OAuthToken",
    ),
    (
        "std.api.oauth.refresh",
        "func(OAuthClient, OAuthToken) returns OAuthToken",
    ),
    (
        "std.api.oauth.revoke",
        "func(OAuthClient, OAuthToken) returns bool",
    ),
    (
        "std.api.oauth.token_access_token",
        "func(OAuthToken) returns string",
    ),
    (
        "std.api.oauth.token_refresh_token",
        "func(OAuthToken) returns string",
    ),
    (
        "std.api.oauth.token_type",
        "func(OAuthToken) returns string",
    ),
    (
        "std.api.oauth.token_expires_at_ms",
        "func(OAuthToken) returns int",
    ),
    (
        "std.api.oauth.token_scope",
        "func(OAuthToken) returns string",
    ),
    ("std.api.tls.config_new", "func(int) returns TlsConfig"),
    ("std.api.tls.config_mode", "func(TlsConfig) returns int"),
    (
        "std.api.tls.server_config",
        "func(string, string) returns TlsConfig",
    ),
    ("std.api.tls.client_config", "func() returns TlsConfig"),
    ("std.api.routing.router", "func() returns Router"),
    ("std.api.routing.router_new", "func() returns Router"),
    ("std.api.routing.route_count", "func(Router) returns int"),
    ("std.api.routing.route_id", "func(Route) returns int"),
    (
        "std.api.routing.route_add",
        "func(Router, int, string) returns Route",
    ),
    (
        "std.api.routing.route_match",
        "func(Router, int, string) returns RouteMatch",
    ),
    ("std.api.routing.match_route_id", "func(RouteMatch) returns int"),
    (
        "std.api.routing.match_param",
        "func(RouteMatch, string) returns string",
    ),
    (
        "std.api.routing.match_param_int",
        "func(RouteMatch, string) returns int",
    ),
    ("std.api.routing.last_conflict", "func() returns string"),
    ("std.api.routing.get", "func(Router, string) returns Route"),
    ("std.api.routing.post", "func(Router, string) returns Route"),
    ("std.api.routing.put", "func(Router, string) returns Route"),
    ("std.api.routing.patch", "func(Router, string) returns Route"),
    ("std.api.routing.delete", "func(Router, string) returns Route"),
    ("std.api.query.type_string", "func() returns int"),
    ("std.api.query.type_int", "func() returns int"),
    ("std.api.query.type_bool", "func() returns int"),
    ("std.api.query.parse", "func(string) returns Query"),
    ("std.api.query.len", "func(Query) returns int"),
    ("std.api.query.has", "func(Query, string) returns bool"),
    ("std.api.query.count", "func(Query, string) returns int"),
    ("std.api.query.first", "func(Query, string) returns string"),
    ("std.api.query.value", "func(Query, string, int) returns string"),
    ("std.api.query.int", "func(Query, string, int) returns int"),
    ("std.api.query.bool", "func(Query, string, int) returns bool"),
    ("std.api.query.schema", "func() returns QuerySchema"),
    (
        "std.api.query.schema_field",
        "func(QuerySchema, string, int, bool, bool) returns QuerySchema",
    ),
    (
        "std.api.query.bind",
        "func(Query, QuerySchema) returns QueryBinding",
    ),
    ("std.api.query.binding_ok", "func(QueryBinding) returns bool"),
    ("std.api.query.binding_error", "func(QueryBinding) returns string"),
    (
        "std.api.query.binding_count",
        "func(QueryBinding, string) returns int",
    ),
    (
        "std.api.query.binding_value",
        "func(QueryBinding, string, int) returns string",
    ),
    (
        "std.api.query.binding_int",
        "func(QueryBinding, string, int) returns int",
    ),
    (
        "std.api.query.binding_bool",
        "func(QueryBinding, string, int) returns bool",
    ),
    ("std.api.query.error_code", "func() returns int"),
    ("std.api.query.error_message", "func() returns string"),
    ("std.api.form.type_string", "func() returns int"),
    ("std.api.form.type_int", "func() returns int"),
    ("std.api.form.type_bool", "func() returns int"),
    ("std.api.form.parse", "func(string) returns Form"),
    ("std.api.form.len", "func(Form) returns int"),
    ("std.api.form.has", "func(Form, string) returns bool"),
    ("std.api.form.count", "func(Form, string) returns int"),
    ("std.api.form.first", "func(Form, string) returns string"),
    ("std.api.form.value", "func(Form, string, int) returns string"),
    ("std.api.form.int", "func(Form, string, int) returns int"),
    ("std.api.form.bool", "func(Form, string, int) returns bool"),
    ("std.api.form.schema", "func() returns FormSchema"),
    (
        "std.api.form.schema_field",
        "func(FormSchema, string, int, bool, bool) returns FormSchema",
    ),
    ("std.api.form.bind", "func(Form, FormSchema) returns FormBinding"),
    ("std.api.form.binding_ok", "func(FormBinding) returns bool"),
    ("std.api.form.binding_error", "func(FormBinding) returns string"),
    (
        "std.api.form.binding_count",
        "func(FormBinding, string) returns int",
    ),
    (
        "std.api.form.binding_value",
        "func(FormBinding, string, int) returns string",
    ),
    (
        "std.api.form.binding_int",
        "func(FormBinding, string, int) returns int",
    ),
    (
        "std.api.form.binding_bool",
        "func(FormBinding, string, int) returns bool",
    ),
    ("std.api.form.error_code", "func() returns int"),
    ("std.api.form.error_message", "func() returns string"),
    (
        "std.api.multipart.parse",
        "func(string, string, int, int, int) returns Multipart",
    ),
    ("std.api.multipart.part_count", "func(Multipart) returns int"),
    ("std.api.multipart.field_count", "func(Multipart) returns int"),
    ("std.api.multipart.file_count", "func(Multipart) returns int"),
    (
        "std.api.multipart.text",
        "func(Multipart, string, int) returns string",
    ),
    (
        "std.api.multipart.part",
        "func(Multipart, int) returns MultipartPart",
    ),
    ("std.api.multipart.part_name", "func(MultipartPart) returns string"),
    (
        "std.api.multipart.part_filename",
        "func(MultipartPart) returns string",
    ),
    (
        "std.api.multipart.part_content_type",
        "func(MultipartPart) returns string",
    ),
    ("std.api.multipart.part_size", "func(MultipartPart) returns int"),
    (
        "std.api.multipart.part_is_file",
        "func(MultipartPart) returns bool",
    ),
    ("std.api.multipart.file_path", "func(MultipartPart) returns string"),
    (
        "std.api.multipart.file_read",
        "func(MultipartPart, int, int) returns string",
    ),
    (
        "std.api.multipart.file_spool_to",
        "func(MultipartPart, string) returns bool",
    ),
    ("std.api.multipart.error_code", "func() returns int"),
    ("std.api.multipart.error_message", "func() returns string"),
    ("std.api.handler.text", "func(string) returns Response"),
    ("std.api.handler.json", "func(string) returns Response"),
    ("std.api.handler.bytes", "func(string) returns Response"),
    ("std.api.handler.status", "func(int) returns Response"),
    (
        "std.api.handler.with_header",
        "func(Response, string, string) returns Response",
    ),
    ("std.api.handler.into_response", "func(Response) returns Response"),
    (
        "std.api.handler.into_text_response",
        "func(string) returns Response",
    ),
    (
        "std.api.handler.into_status_response",
        "func(int) returns Response",
    ),
    ("std.api.handler.error", "func(int, string) returns HandlerError"),
    (
        "std.api.handler.error_response",
        "func(HandlerError) returns Response",
    ),
    ("std.api.handler.error_code", "func(HandlerError) returns int"),
    (
        "std.api.handler.error_message",
        "func(HandlerError) returns string",
    ),
    ("std.api.handler.last_error_message", "func() returns string"),
    (
        "std.api.handler.register_sync",
        "func(int, Response) returns HandlerHandle",
    ),
    (
        "std.api.handler.register_async",
        "func(int, Response) returns AsyncHandlerHandle",
    ),
    (
        "std.api.handler.register_sync_callback",
        "func(int, fn(Request) returns Response) returns HandlerHandle",
    ),
    (
        "std.api.handler.register_async_callback",
        "func(int, fn(Request) returns Task<Response>) returns AsyncHandlerHandle",
    ),
    (
        "std.api.handler.dispatch_sync",
        "func(HandlerHandle, Request) returns Response",
    ),
    (
        "std.api.handler.dispatch_async",
        "func(AsyncHandlerHandle, Request) returns Response",
    ),
    ("std.api.cors.policy", "func() returns CorsPolicy"),
    ("std.api.cors.permissive", "func() returns CorsPolicy"),
    (
        "std.api.cors.allow_origin",
        "func(CorsPolicy, string) returns CorsPolicy",
    ),
    (
        "std.api.cors.allow_method",
        "func(CorsPolicy, int) returns CorsPolicy",
    ),
    (
        "std.api.cors.allow_header",
        "func(CorsPolicy, string) returns CorsPolicy",
    ),
    (
        "std.api.cors.expose_header",
        "func(CorsPolicy, string) returns CorsPolicy",
    ),
    (
        "std.api.cors.allow_credentials",
        "func(CorsPolicy, bool) returns CorsPolicy",
    ),
    ("std.api.cors.max_age", "func(CorsPolicy, int) returns CorsPolicy"),
    (
        "std.api.cors.middleware",
        "func(CorsPolicy) returns MiddlewareHandle",
    ),
    ("std.api.cors.is_preflight", "func(Request) returns bool"),
    (
        "std.api.cors.preflight",
        "func(CorsPolicy, Request) returns Response",
    ),
    (
        "std.api.cors.apply",
        "func(CorsPolicy, Request, Response) returns Response",
    ),
    (
        "std.api.cors.allowed_origin",
        "func(CorsPolicy, string) returns string",
    ),
    ("std.api.middleware.chain", "func() returns MiddlewareChain"),
    ("std.api.middleware.chain_new", "func() returns MiddlewareChain"),
    ("std.api.middleware.chain_len", "func(MiddlewareChain) returns int"),
    (
        "std.api.middleware.register_sync",
        "func(string, string) returns MiddlewareHandle",
    ),
    (
        "std.api.middleware.register_sync_short_circuit",
        "func(string, string, Response) returns MiddlewareHandle",
    ),
    (
        "std.api.middleware.register_async",
        "func(string, string) returns AsyncMiddlewareHandle",
    ),
    (
        "std.api.middleware.register_async_short_circuit",
        "func(string, string, Response) returns AsyncMiddlewareHandle",
    ),
    (
        "std.api.middleware.register_logging",
        "func(string) returns MiddlewareHandle",
    ),
    (
        "std.api.middleware.logging_len",
        "func(MiddlewareHandle) returns int",
    ),
    (
        "std.api.middleware.logging_line",
        "func(MiddlewareHandle, int) returns string",
    ),
    (
        "std.api.middleware.logging_request_id",
        "func(MiddlewareHandle, int) returns string",
    ),
    (
        "std.api.middleware.register_rate_limit",
        "func(string, int, int, string, bool) returns MiddlewareHandle",
    ),
    (
        "std.api.middleware.rate_limit_update",
        "func(MiddlewareHandle, int, int) returns bool",
    ),
    (
        "std.api.middleware.register_api_key",
        "func(string) returns MiddlewareHandle",
    ),
    (
        "std.api.middleware.api_key_add",
        "func(MiddlewareHandle, string, int) returns bool",
    ),
    (
        "std.api.middleware.api_key_revoke",
        "func(MiddlewareHandle, string) returns bool",
    ),
    (
        "std.api.middleware.register_security_headers",
        "func() returns MiddlewareHandle",
    ),
    (
        "std.api.middleware.register_compression",
        "func(int) returns MiddlewareHandle",
    ),
    (
        "std.api.middleware.security_headers_configure",
        "func(MiddlewareHandle, string, string, bool, bool, bool) returns bool",
    ),
    (
        "std.api.middleware.security_headers_route",
        "func(MiddlewareHandle, string, string, string) returns bool",
    ),
    (
        "std.api.middleware.use_sync",
        "func(MiddlewareChain, MiddlewareHandle) returns MiddlewareChain",
    ),
    (
        "std.api.middleware.use_async",
        "func(MiddlewareChain, AsyncMiddlewareHandle) returns MiddlewareChain",
    ),
    (
        "std.api.middleware.execute_sync",
        "func(MiddlewareChain, Request, Response) returns Response",
    ),
    (
        "std.api.middleware.execute_async",
        "func(MiddlewareChain, Request, Response) returns Response",
    ),
    ("std.api.middleware.last_trace", "func() returns MiddlewareTrace"),
    ("std.api.middleware.trace_len", "func(MiddlewareTrace) returns int"),
    (
        "std.api.middleware.trace_event",
        "func(MiddlewareTrace, int) returns string",
    ),
    (
        "std.api.middleware.trace_short_circuited",
        "func(MiddlewareTrace) returns bool",
    ),
    (
        "std.api.validation.schema",
        "func() returns ValidationSchema",
    ),
    (
        "std.api.validation.field",
        "func(ValidationSchema, string, int, bool) returns ValidationSchema",
    ),
    (
        "std.api.validation.min_length",
        "func(ValidationSchema, string, int) returns ValidationSchema",
    ),
    (
        "std.api.validation.max_length",
        "func(ValidationSchema, string, int) returns ValidationSchema",
    ),
    (
        "std.api.validation.range",
        "func(ValidationSchema, string, int, int) returns ValidationSchema",
    ),
    (
        "std.api.validation.regex",
        "func(ValidationSchema, string, string) returns ValidationSchema",
    ),
    (
        "std.api.validation.validate_json",
        "func(ValidationSchema, string) returns ValidationResult",
    ),
    (
        "std.api.validation.validate_form",
        "func(ValidationSchema, Form) returns ValidationResult",
    ),
    (
        "std.api.validation.result_ok",
        "func(ValidationResult) returns bool",
    ),
    (
        "std.api.validation.result_count",
        "func(ValidationResult) returns int",
    ),
    (
        "std.api.validation.result_field",
        "func(ValidationResult, int) returns string",
    ),
    (
        "std.api.validation.result_code",
        "func(ValidationResult, int) returns string",
    ),
    (
        "std.api.validation.result_message",
        "func(ValidationResult, int) returns string",
    ),
    (
        "std.api.validation.result_problem_json",
        "func(ValidationResult) returns string",
    ),
    (
        "std.api.validation.result_response",
        "func(ValidationResult) returns Response",
    ),
    ("std.api.validation.error_code", "func() returns int"),
    ("std.api.validation.error_message", "func() returns string"),
    (
        "std.api.errors.new",
        "func(int, string, string) returns ApiError",
    ),
    (
        "std.api.errors.internal_error",
        "func(string, string) returns ApiError",
    ),
    ("std.api.errors.status", "func(ApiError) returns int"),
    ("std.api.errors.code", "func(ApiError) returns string"),
    ("std.api.errors.message", "func(ApiError) returns string"),
    ("std.api.errors.response", "func(ApiError) returns Response"),
    (
        "std.api.errors.exception_middleware",
        "func(int, string, string) returns MiddlewareHandle",
    ),
    ("std.api.errors.last_code", "func() returns int"),
    ("std.api.errors.last_message", "func() returns string"),
    (
        "std.api.security.csrf_policy",
        "func() returns CsrfPolicy",
    ),
    (
        "std.api.security.csrf_allow_origin",
        "func(CsrfPolicy, string) returns CsrfPolicy",
    ),
    (
        "std.api.security.csrf_origin_count",
        "func(CsrfPolicy) returns int",
    ),
    (
        "std.api.security.csrf_middleware",
        "func(CsrfPolicy) returns MiddlewareHandle",
    ),
    (
        "std.api.security.ssrf_policy",
        "func() returns SsrfPolicy",
    ),
    (
        "std.api.security.ssrf_allow_private_networks",
        "func(SsrfPolicy, bool) returns SsrfPolicy",
    ),
    (
        "std.api.security.ssrf_allows",
        "func(SsrfPolicy, string) returns bool",
    ),
    (
        "std.api.session.memory_store",
        "func() returns SessionStore",
    ),
    (
        "std.api.session.redis_store",
        "func(RedisConnection, string) returns SessionStore",
    ),
    (
        "std.api.session.store_kind",
        "func(SessionStore) returns string",
    ),
    (
        "std.api.session.create",
        "func(SessionStore, string, int, int, bool) returns Session",
    ),
    (
        "std.api.session.lookup",
        "func(SessionStore, string) returns Session",
    ),
    ("std.api.session.id", "func(Session) returns string"),
    ("std.api.session.value", "func(Session) returns string"),
    (
        "std.api.session.created_at_ms",
        "func(Session) returns int",
    ),
    (
        "std.api.session.expires_at_ms",
        "func(Session) returns int",
    ),
    ("std.api.session.is_valid", "func(Session) returns bool"),
    (
        "std.api.session.revoke",
        "func(SessionStore, string) returns bool",
    ),
    ("std.api.session.error_code", "func() returns int"),
    (
        "std.api.session.error_message",
        "func() returns string",
    ),
    (
        "std.api.websocket.client_new",
        "func() returns WebSocketClient",
    ),
    (
        "std.api.websocket.client_set_per_message_deflate",
        "func(WebSocketClient, bool) returns bool",
    ),
    (
        "std.api.websocket.client_set_max_message_bytes",
        "func(WebSocketClient, int) returns bool",
    ),
    (
        "std.api.websocket.client_set_reconnect",
        "func(WebSocketClient, int, int) returns bool",
    ),
    (
        "std.api.websocket.client_allow_private_networks",
        "func(WebSocketClient, bool) returns bool",
    ),
    (
        "std.api.websocket.client_connect",
        "func(WebSocketClient, string) returns task<WebSocket>",
    ),
    (
        "std.api.websocket.server_new",
        "func() returns WebSocketServer",
    ),
    (
        "std.api.websocket.server_route",
        "func(WebSocketServer, Route) returns bool",
    ),
    (
        "std.api.websocket.server_listen",
        "func(WebSocketServer, int) returns bool",
    ),
    (
        "std.api.websocket.server_local_port",
        "func(WebSocketServer) returns int",
    ),
    (
        "std.api.websocket.server_set_per_message_deflate",
        "func(WebSocketServer, bool) returns bool",
    ),
    (
        "std.api.websocket.server_set_max_message_bytes",
        "func(WebSocketServer, int) returns bool",
    ),
    (
        "std.api.websocket.server_accept",
        "func(WebSocketServer) returns task<WebSocket>",
    ),
    (
        "std.api.websocket.connection_peer_port",
        "func(WebSocket) returns int",
    ),
    (
        "std.api.websocket.connection_receive",
        "func(WebSocket) returns task<WebSocketMessage>",
    ),
    (
        "std.api.websocket.connection_send_text",
        "func(WebSocket, string) returns task<int>",
    ),
    (
        "std.api.websocket.connection_send_binary_base64",
        "func(WebSocket, string) returns task<int>",
    ),
    (
        "std.api.websocket.connection_ping",
        "func(WebSocket, string) returns task<int>",
    ),
    (
        "std.api.websocket.connection_close",
        "func(WebSocket, int, string) returns task<int>",
    ),
    (
        "std.api.websocket.message_kind",
        "func(WebSocketMessage) returns int",
    ),
    (
        "std.api.websocket.message_len",
        "func(WebSocketMessage) returns int",
    ),
    (
        "std.api.websocket.message_text",
        "func(WebSocketMessage) returns string",
    ),
    (
        "std.api.websocket.message_base64",
        "func(WebSocketMessage) returns string",
    ),
    (
        "std.api.websocket.message_release",
        "func(WebSocketMessage) returns bool",
    ),
    ("std.api.sse.server_new", "func() returns SseServer"),
    (
        "std.api.sse.server_response",
        "func(SseServer) returns Response",
    ),
    ("std.api.sse.server_listen", "func(SseServer, int) returns bool"),
    ("std.api.sse.server_local_port", "func(SseServer) returns int"),
    (
        "std.api.sse.server_set_heartbeat_interval",
        "func(SseServer, int) returns bool",
    ),
    (
        "std.api.sse.server_set_replay_capacity",
        "func(SseServer, int) returns bool",
    ),
    (
        "std.api.sse.server_set_max_event_bytes",
        "func(SseServer, int) returns bool",
    ),
    (
        "std.api.sse.server_accept",
        "func(SseServer) returns task<SseConnection>",
    ),
    (
        "std.api.sse.server_publish",
        "func(SseServer, SseEvent) returns task<int>",
    ),
    (
        "std.api.sse.event_new",
        "func(string, string, string, int) returns SseEvent",
    ),
    ("std.api.sse.event_id", "func(SseEvent) returns string"),
    ("std.api.sse.event_type", "func(SseEvent) returns string"),
    ("std.api.sse.event_data", "func(SseEvent) returns string"),
    ("std.api.sse.event_retry_ms", "func(SseEvent) returns int"),
    ("std.api.sse.event_release", "func(SseEvent) returns bool"),
    (
        "std.api.sse.connection_peer_port",
        "func(SseConnection) returns int",
    ),
    (
        "std.api.sse.connection_last_event_id",
        "func(SseConnection) returns string",
    ),
    (
        "std.api.sse.connection_send",
        "func(SseConnection, SseEvent) returns task<int>",
    ),
    (
        "std.api.sse.connection_heartbeat",
        "func(SseConnection) returns task<int>",
    ),
    (
        "std.api.sse.connection_close",
        "func(SseConnection) returns task<int>",
    ),
];

pub const STD_TIME_PUBLIC_TYPES: &[(&str, &str)] = &[
    ("std.time.Duration", "record Duration"),
    ("std.time.Instant", "record Instant"),
    ("std.time.UtcDateTime", "record UtcDateTime"),
];

pub const STD_TIME_PUBLIC_FUNCTIONS: &[(&str, &str)] = &[
    ("std.time.time_now_millis", "func() returns int"),
    ("std.time.time_now_secs", "func() returns int"),
    ("std.time.sleep_ms", "func(int) returns unit"),
    ("std.time.monotonic_millis", "func() returns int"),
    ("std.time.monotonic_nanos", "func() returns int"),
    ("std.time.duration_ms", "func(int) returns Duration"),
    ("std.time.duration_secs", "func(int) returns Duration"),
    ("std.time.duration_millis", "func(Duration) returns int"),
    ("std.time.duration_secs_value", "func(Duration) returns int"),
    (
        "std.time.duration_add",
        "func(Duration, Duration) returns Duration",
    ),
    (
        "std.time.duration_sub",
        "func(Duration, Duration) returns Duration",
    ),
    ("std.time.instant_now", "func() returns Instant"),
    ("std.time.instant_elapsed_ms", "func(Instant) returns int"),
    ("std.time.instant_add", "func(Instant, Duration) returns Instant"),
    ("std.time.instant_has_elapsed", "func(Instant) returns bool"),
    ("std.time.sleep", "func(Duration) returns unit"),
    ("std.time.unix_to_utc", "func(int) returns UtcDateTime"),
    ("std.time.utc_year", "func(UtcDateTime) returns int"),
    ("std.time.utc_month", "func(UtcDateTime) returns int"),
    ("std.time.utc_day", "func(UtcDateTime) returns int"),
    ("std.time.utc_hour", "func(UtcDateTime) returns int"),
    ("std.time.utc_minute", "func(UtcDateTime) returns int"),
    ("std.time.utc_second", "func(UtcDateTime) returns int"),
];

pub const STD_RANGE_PUBLIC_TYPES: &[(&str, &str)] = &[("std.range.Range", "record Range")];

pub const STD_RANGE_PUBLIC_FUNCTIONS: &[(&str, &str)] = &[
    ("std.range.create", "func(int, int, bool) returns Range"),
    ("std.range.len", "func(Range) returns int"),
    ("std.range.at", "func(Range, int) returns int"),
    ("std.range.eq", "func(Range, Range) returns bool"),
    ("std.range.start", "func(Range) returns int"),
    ("std.range.end", "func(Range) returns int"),
    ("std.range.is_inclusive", "func(Range) returns bool"),
    ("std.range.iter", "func(Range) returns Iterator<int>"),
];
