#[derive(Clone, Copy, Debug)]
pub struct HostCallSpec {
    pub name: &'static str,
    pub function: HostFunction,
}

pub const HOST_CALLS: &[HostCallSpec] = &[
    HostCallSpec {
        name: "spectra.api.version.major",
        function: api_version_major,
    },
    HostCallSpec {
        name: "spectra.api.version.minor",
        function: api_version_minor,
    },
    HostCallSpec {
        name: "spectra.api.version.patch",
        function: api_version_patch,
    },
    HostCallSpec { name: "spectra.api.health.startup_complete", function: health::startup_complete },
    HostCallSpec { name: "spectra.api.health.startup_failed", function: health::startup_failed },
    HostCallSpec {
        name: "spectra.api.http.method_name",
        function: http::method_name,
    },
    HostCallSpec {
        name: "spectra.api.http.method_allows_body",
        function: http::method_allows_body,
    },
    HostCallSpec {
        name: "spectra.api.http.method_is_safe",
        function: http::method_is_safe,
    },
    HostCallSpec {
        name: "spectra.api.http.method_get",
        function: http::method_get,
    },
    HostCallSpec {
        name: "spectra.api.http.method_head",
        function: http::method_head,
    },
    HostCallSpec {
        name: "spectra.api.http.method_post",
        function: http::method_post,
    },
    HostCallSpec {
        name: "spectra.api.http.method_put",
        function: http::method_put,
    },
    HostCallSpec {
        name: "spectra.api.http.method_patch",
        function: http::method_patch,
    },
    HostCallSpec {
        name: "spectra.api.http.method_delete",
        function: http::method_delete,
    },
    HostCallSpec {
        name: "spectra.api.http.method_options",
        function: http::method_options,
    },
    HostCallSpec {
        name: "spectra.api.http.status_reason",
        function: http::status_reason,
    },
    HostCallSpec {
        name: "spectra.api.http.status_class",
        function: http::status_class,
    },
    HostCallSpec {
        name: "spectra.api.http.status_is_success",
        function: http::status_is_success,
    },
    HostCallSpec {
        name: "spectra.api.http.status",
        function: http::status,
    },
    HostCallSpec {
        name: "spectra.api.http.status_continue",
        function: http::status_continue,
    },
    HostCallSpec {
        name: "spectra.api.http.status_switching_protocols",
        function: http::status_switching_protocols,
    },
    HostCallSpec {
        name: "spectra.api.http.status_ok",
        function: http::status_ok,
    },
    HostCallSpec {
        name: "spectra.api.http.status_created",
        function: http::status_created,
    },
    HostCallSpec {
        name: "spectra.api.http.status_accepted",
        function: http::status_accepted,
    },
    HostCallSpec {
        name: "spectra.api.http.status_no_content",
        function: http::status_no_content,
    },
    HostCallSpec {
        name: "spectra.api.http.status_moved_permanently",
        function: http::status_moved_permanently,
    },
    HostCallSpec {
        name: "spectra.api.http.status_found",
        function: http::status_found,
    },
    HostCallSpec {
        name: "spectra.api.http.status_not_modified",
        function: http::status_not_modified,
    },
    HostCallSpec {
        name: "spectra.api.http.status_bad_request",
        function: http::status_bad_request,
    },
    HostCallSpec {
        name: "spectra.api.http.status_unauthorized",
        function: http::status_unauthorized,
    },
    HostCallSpec {
        name: "spectra.api.http.status_forbidden",
        function: http::status_forbidden,
    },
    HostCallSpec {
        name: "spectra.api.http.status_not_found",
        function: http::status_not_found,
    },
    HostCallSpec {
        name: "spectra.api.http.status_method_not_allowed",
        function: http::status_method_not_allowed,
    },
    HostCallSpec {
        name: "spectra.api.http.status_conflict",
        function: http::status_conflict,
    },
    HostCallSpec {
        name: "spectra.api.http.status_unsupported_media_type",
        function: http::status_unsupported_media_type,
    },
    HostCallSpec {
        name: "spectra.api.http.status_unprocessable_content",
        function: http::status_unprocessable_content,
    },
    HostCallSpec {
        name: "spectra.api.http.status_too_many_requests",
        function: http::status_too_many_requests,
    },
    HostCallSpec {
        name: "spectra.api.http.status_internal_server_error",
        function: http::status_internal_server_error,
    },
    HostCallSpec {
        name: "spectra.api.http.status_bad_gateway",
        function: http::status_bad_gateway,
    },
    HostCallSpec {
        name: "spectra.api.http.status_service_unavailable",
        function: http::status_service_unavailable,
    },
    HostCallSpec {
        name: "spectra.api.http.status_gateway_timeout",
        function: http::status_gateway_timeout,
    },
    HostCallSpec {
        name: "spectra.api.http.header_name_is_valid",
        function: http::header_name_is_valid,
    },
    HostCallSpec {
        name: "spectra.api.http.header_value_is_valid",
        function: http::header_value_is_valid,
    },
    HostCallSpec {
        name: "spectra.api.http.request_new",
        function: http::request_new,
    },
    HostCallSpec {
        name: "spectra.api.http.request",
        function: http::request,
    },
    HostCallSpec {
        name: "spectra.api.http.request_method",
        function: http::request_method,
    },
    HostCallSpec {
        name: "spectra.api.http.request_path",
        function: http::request_path,
    },
    HostCallSpec {
        name: "spectra.api.http.request_body",
        function: http::request_body,
    },
    HostCallSpec {
        name: "spectra.api.http.request_header",
        function: http::request_header,
    },
    HostCallSpec {
        name: "spectra.api.http.request_with_header",
        function: http::request_with_header,
    },
    HostCallSpec {
        name: "spectra.api.http.request_with_body",
        function: http::request_with_body,
    },
    HostCallSpec {
        name: "spectra.api.http.request_cookie",
        function: http::request_cookie,
    },
    HostCallSpec {
        name: "spectra.api.http.response_new",
        function: http::response_new,
    },
    HostCallSpec {
        name: "spectra.api.http.response",
        function: http::response_new,
    },
    HostCallSpec {
        name: "spectra.api.http.response_status",
        function: http::response_status,
    },
    HostCallSpec {
        name: "spectra.api.http.response_header",
        function: http::response_header,
    },
    HostCallSpec {
        name: "spectra.api.http.response_body_len",
        function: http::response_body_len,
    },
    HostCallSpec {
        name: "spectra.api.http.header",
        function: http::header,
    },
    HostCallSpec {
        name: "spectra.api.http.header_name",
        function: http::header_name,
    },
    HostCallSpec {
        name: "spectra.api.http.header_value",
        function: http::header_value,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie",
        function: http::cookie,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_name",
        function: http::cookie_name,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_value",
        function: http::cookie_value,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_with_options",
        function: http::cookie_with_options,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_path",
        function: http::cookie_path,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_domain",
        function: http::cookie_domain,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_max_age",
        function: http::cookie_max_age,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_secure",
        function: http::cookie_secure,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_http_only",
        function: http::cookie_http_only,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_same_site",
        function: http::cookie_same_site,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_header",
        function: http::cookie_header,
    },
    HostCallSpec {
        name: "spectra.api.http.response_with_cookie",
        function: http::response_with_cookie,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_sign",
        function: http::cookie_sign,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_verify",
        function: http::cookie_verify,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_is_expired",
        function: http::cookie_is_expired,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_error_code",
        function: http::cookie_error_code,
    },
    HostCallSpec {
        name: "spectra.api.http.cookie_error_message",
        function: http::cookie_error_message,
    },
    HostCallSpec {
        name: "spectra.api.server.new",
        function: server::server_new,
    },
    HostCallSpec {
        name: "spectra.api.server.listen",
        function: server::server_listen,
    },
    HostCallSpec {
        name: "spectra.api.server.serve",
        function: server::server_serve,
    },
    HostCallSpec {
        name: "spectra.api.server.state",
        function: server::server_state,
    },
    HostCallSpec {
        name: "spectra.api.server.shutdown",
        function: server::server_shutdown,
    },
    HostCallSpec {
        name: "spectra.api.server.local_port",
        function: server::server_local_port,
    },
    HostCallSpec {
        name: "spectra.api.server.signal",
        function: server::server_signal,
    },
    HostCallSpec {
        name: "spectra.api.server.stats",
        function: server::server_stats,
    },
    HostCallSpec {
        name: "spectra.api.server.set_max_body_bytes",
        function: server::server_set_max_body_bytes,
    },
    HostCallSpec {
        name: "spectra.api.server.set_read_timeout",
        function: server::server_set_read_timeout,
    },
    HostCallSpec {
        name: "spectra.api.server.set_idle_timeout",
        function: server::server_set_idle_timeout,
    },
    HostCallSpec {
        name: "spectra.api.client.new",
        function: client::client_new,
    },
    HostCallSpec {
        name: "spectra.api.client.request",
        function: client::client_request,
    },
    HostCallSpec {
        name: "spectra.api.client.timeout_ms",
        function: client::client_timeout_ms,
    },
    HostCallSpec {
        name: "spectra.api.client.set_ssrf_policy",
        function: client::client_set_ssrf_policy,
    },
    HostCallSpec {
        name: "spectra.api.json.validate",
        function: json::json_validate,
    },
    HostCallSpec {
        name: "spectra.api.json.kind",
        function: json::json_kind,
    },
    HostCallSpec {
        name: "spectra.api.jwt.sign",
        function: jwt::jwt_sign,
    },
    HostCallSpec {
        name: "spectra.api.jwt.verify",
        function: jwt::jwt_verify,
    },
    HostCallSpec {
        name: "spectra.api.oauth.client_new",
        function: oauth::client_new,
    },
    HostCallSpec {
        name: "spectra.api.oauth.client_set_revocation_url",
        function: oauth::client_set_revocation_url,
    },
    HostCallSpec {
        name: "spectra.api.oauth.authorization_url",
        function: oauth::authorization_url,
    },
    HostCallSpec {
        name: "spectra.api.oauth.exchange_code",
        function: oauth::exchange_code,
    },
    HostCallSpec {
        name: "spectra.api.oauth.refresh",
        function: oauth::refresh,
    },
    HostCallSpec {
        name: "spectra.api.oauth.revoke",
        function: oauth::revoke,
    },
    HostCallSpec {
        name: "spectra.api.oauth.token_access_token",
        function: oauth::token_access_token,
    },
    HostCallSpec {
        name: "spectra.api.oauth.token_refresh_token",
        function: oauth::token_refresh_token,
    },
    HostCallSpec {
        name: "spectra.api.oauth.token_type",
        function: oauth::token_type,
    },
    HostCallSpec {
        name: "spectra.api.oauth.token_expires_at_ms",
        function: oauth::token_expires_at_ms,
    },
    HostCallSpec {
        name: "spectra.api.oauth.token_scope",
        function: oauth::token_scope,
    },
    HostCallSpec {
        name: "spectra.api.tls.config_new",
        function: tls::tls_config_new,
    },
    HostCallSpec {
        name: "spectra.api.tls.client_config",
        function: tls::tls_client_config,
    },
    HostCallSpec {
        name: "spectra.api.tls.config_mode",
        function: tls::tls_config_mode,
    },
    HostCallSpec {
        name: "spectra.api.routing.router_new",
        function: routing::router_new,
    },
    HostCallSpec {
        name: "spectra.api.routing.route_count",
        function: routing::route_count,
    },
    HostCallSpec {
        name: "spectra.api.routing.route_id",
        function: routing::route_id,
    },
    HostCallSpec {
        name: "spectra.api.routing.route_add",
        function: routing::route_add,
    },
    HostCallSpec {
        name: "spectra.api.routing.get",
        function: routing::get,
    },
    HostCallSpec {
        name: "spectra.api.routing.post",
        function: routing::post,
    },
    HostCallSpec {
        name: "spectra.api.routing.put",
        function: routing::put,
    },
    HostCallSpec {
        name: "spectra.api.routing.patch",
        function: routing::patch,
    },
    HostCallSpec {
        name: "spectra.api.routing.delete",
        function: routing::delete,
    },
    HostCallSpec {
        name: "spectra.api.routing.route_match",
        function: routing::route_match,
    },
    HostCallSpec {
        name: "spectra.api.routing.match_route_id",
        function: routing::match_route_id,
    },
    HostCallSpec {
        name: "spectra.api.routing.match_param",
        function: routing::match_param,
    },
    HostCallSpec {
        name: "spectra.api.routing.match_param_int",
        function: routing::match_param_int,
    },
    HostCallSpec {
        name: "spectra.api.routing.last_conflict",
        function: routing::last_conflict,
    },
    HostCallSpec {
        name: "spectra.api.query.type_string",
        function: query::type_string,
    },
    HostCallSpec {
        name: "spectra.api.query.type_int",
        function: query::type_int,
    },
    HostCallSpec {
        name: "spectra.api.query.type_bool",
        function: query::type_bool,
    },
    HostCallSpec {
        name: "spectra.api.query.parse",
        function: query::parse,
    },
    HostCallSpec {
        name: "spectra.api.query.len",
        function: query::len,
    },
    HostCallSpec {
        name: "spectra.api.query.has",
        function: query::has,
    },
    HostCallSpec {
        name: "spectra.api.query.count",
        function: query::count,
    },
    HostCallSpec {
        name: "spectra.api.query.first",
        function: query::first,
    },
    HostCallSpec {
        name: "spectra.api.query.value",
        function: query::value,
    },
    HostCallSpec {
        name: "spectra.api.query.int",
        function: query::int,
    },
    HostCallSpec {
        name: "spectra.api.query.bool",
        function: query::bool,
    },
    HostCallSpec {
        name: "spectra.api.query.schema",
        function: query::schema,
    },
    HostCallSpec {
        name: "spectra.api.query.schema_field",
        function: query::schema_field,
    },
    HostCallSpec {
        name: "spectra.api.query.bind",
        function: query::bind,
    },
    HostCallSpec {
        name: "spectra.api.query.binding_ok",
        function: query::binding_ok,
    },
    HostCallSpec {
        name: "spectra.api.query.binding_error",
        function: query::binding_error,
    },
    HostCallSpec {
        name: "spectra.api.query.binding_count",
        function: query::binding_count,
    },
    HostCallSpec {
        name: "spectra.api.query.binding_value",
        function: query::binding_value,
    },
    HostCallSpec {
        name: "spectra.api.query.binding_int",
        function: query::binding_int,
    },
    HostCallSpec {
        name: "spectra.api.query.binding_bool",
        function: query::binding_bool,
    },
    HostCallSpec {
        name: "spectra.api.query.error_code",
        function: query::error_code,
    },
    HostCallSpec {
        name: "spectra.api.query.error_message",
        function: query::error_message,
    },
    HostCallSpec {
        name: "spectra.api.form.type_string",
        function: form::type_string,
    },
    HostCallSpec {
        name: "spectra.api.form.type_int",
        function: form::type_int,
    },
    HostCallSpec {
        name: "spectra.api.form.type_bool",
        function: form::type_bool,
    },
    HostCallSpec {
        name: "spectra.api.form.parse",
        function: form::parse,
    },
    HostCallSpec {
        name: "spectra.api.form.len",
        function: form::len,
    },
    HostCallSpec {
        name: "spectra.api.form.has",
        function: form::has,
    },
    HostCallSpec {
        name: "spectra.api.form.count",
        function: form::count,
    },
    HostCallSpec {
        name: "spectra.api.form.first",
        function: form::first,
    },
    HostCallSpec {
        name: "spectra.api.form.value",
        function: form::value,
    },
    HostCallSpec {
        name: "spectra.api.form.int",
        function: form::int,
    },
    HostCallSpec {
        name: "spectra.api.form.bool",
        function: form::bool,
    },
    HostCallSpec {
        name: "spectra.api.form.schema",
        function: form::schema,
    },
    HostCallSpec {
        name: "spectra.api.form.schema_field",
        function: form::schema_field,
    },
    HostCallSpec {
        name: "spectra.api.form.bind",
        function: form::bind,
    },
    HostCallSpec {
        name: "spectra.api.form.binding_ok",
        function: form::binding_ok,
    },
    HostCallSpec {
        name: "spectra.api.form.binding_error",
        function: form::binding_error,
    },
    HostCallSpec {
        name: "spectra.api.form.binding_count",
        function: form::binding_count,
    },
    HostCallSpec {
        name: "spectra.api.form.binding_value",
        function: form::binding_value,
    },
    HostCallSpec {
        name: "spectra.api.form.binding_int",
        function: form::binding_int,
    },
    HostCallSpec {
        name: "spectra.api.form.binding_bool",
        function: form::binding_bool,
    },
    HostCallSpec {
        name: "spectra.api.form.error_code",
        function: form::error_code,
    },
    HostCallSpec {
        name: "spectra.api.form.error_message",
        function: form::error_message,
    },
    HostCallSpec {
        name: "spectra.api.multipart.parse",
        function: multipart::parse,
    },
    HostCallSpec {
        name: "spectra.api.multipart.part_count",
        function: multipart::part_count,
    },
    HostCallSpec {
        name: "spectra.api.multipart.field_count",
        function: multipart::field_count,
    },
    HostCallSpec {
        name: "spectra.api.multipart.file_count",
        function: multipart::file_count,
    },
    HostCallSpec {
        name: "spectra.api.multipart.text",
        function: multipart::text,
    },
    HostCallSpec {
        name: "spectra.api.multipart.part",
        function: multipart::part,
    },
    HostCallSpec {
        name: "spectra.api.multipart.part_name",
        function: multipart::part_name,
    },
    HostCallSpec {
        name: "spectra.api.multipart.part_filename",
        function: multipart::part_filename,
    },
    HostCallSpec {
        name: "spectra.api.multipart.part_content_type",
        function: multipart::part_content_type,
    },
    HostCallSpec {
        name: "spectra.api.multipart.part_size",
        function: multipart::part_size,
    },
    HostCallSpec {
        name: "spectra.api.multipart.part_is_file",
        function: multipart::part_is_file,
    },
    HostCallSpec {
        name: "spectra.api.multipart.file_path",
        function: multipart::file_path,
    },
    HostCallSpec {
        name: "spectra.api.multipart.file_read",
        function: multipart::file_read,
    },
    HostCallSpec {
        name: "spectra.api.multipart.file_spool_to",
        function: multipart::file_spool_to,
    },
    HostCallSpec {
        name: "spectra.api.multipart.error_code",
        function: multipart::error_code,
    },
    HostCallSpec {
        name: "spectra.api.multipart.error_message",
        function: multipart::error_message,
    },
    HostCallSpec {
        name: "spectra.api.handler.text",
        function: handler::text,
    },
    HostCallSpec {
        name: "spectra.api.handler.json",
        function: handler::json,
    },
    HostCallSpec {
        name: "spectra.api.handler.bytes",
        function: handler::bytes,
    },
    HostCallSpec {
        name: "spectra.api.handler.status",
        function: handler::status,
    },
    HostCallSpec {
        name: "spectra.api.handler.with_header",
        function: handler::with_header,
    },
    HostCallSpec {
        name: "spectra.api.handler.into_response",
        function: handler::into_response,
    },
    HostCallSpec {
        name: "spectra.api.handler.into_text_response",
        function: handler::into_text_response,
    },
    HostCallSpec {
        name: "spectra.api.handler.into_status_response",
        function: handler::into_status_response,
    },
    HostCallSpec {
        name: "spectra.api.handler.error",
        function: handler::error,
    },
    HostCallSpec {
        name: "spectra.api.handler.error_response",
        function: handler::error_response,
    },
    HostCallSpec {
        name: "spectra.api.handler.error_code",
        function: handler::error_code,
    },
    HostCallSpec {
        name: "spectra.api.handler.error_message",
        function: handler::error_message,
    },
    HostCallSpec {
        name: "spectra.api.handler.last_error_message",
        function: handler::last_error_message,
    },
    HostCallSpec {
        name: "spectra.api.handler.register_sync",
        function: handler::register_sync,
    },
    HostCallSpec {
        name: "spectra.api.handler.register_async",
        function: handler::register_async,
    },
    HostCallSpec {
        name: "spectra.api.handler.register_sync_callback",
        function: handler::register_sync_callback,
    },
    HostCallSpec {
        name: "spectra.api.handler.register_async_callback",
        function: handler::register_async_callback,
    },
    HostCallSpec {
        name: "spectra.api.handler.dispatch_sync",
        function: handler::dispatch_sync,
    },
    HostCallSpec {
        name: "spectra.api.handler.dispatch_async",
        function: handler::dispatch_async,
    },
    HostCallSpec {
        name: "spectra.api.cors.policy",
        function: cors::policy,
    },
    HostCallSpec {
        name: "spectra.api.cors.permissive",
        function: cors::permissive,
    },
    HostCallSpec {
        name: "spectra.api.cors.allow_origin",
        function: cors::allow_origin,
    },
    HostCallSpec {
        name: "spectra.api.cors.allow_method",
        function: cors::allow_method,
    },
    HostCallSpec {
        name: "spectra.api.cors.allow_header",
        function: cors::allow_header,
    },
    HostCallSpec {
        name: "spectra.api.cors.expose_header",
        function: cors::expose_header,
    },
    HostCallSpec {
        name: "spectra.api.cors.allow_credentials",
        function: cors::allow_credentials,
    },
    HostCallSpec {
        name: "spectra.api.cors.max_age",
        function: cors::max_age,
    },
    HostCallSpec {
        name: "spectra.api.cors.middleware",
        function: cors::middleware,
    },
    HostCallSpec {
        name: "spectra.api.cors.is_preflight",
        function: cors::is_preflight,
    },
    HostCallSpec {
        name: "spectra.api.cors.preflight",
        function: cors::preflight,
    },
    HostCallSpec {
        name: "spectra.api.cors.apply",
        function: cors::apply,
    },
    HostCallSpec {
        name: "spectra.api.cors.allowed_origin",
        function: cors::allowed_origin,
    },
    HostCallSpec {
        name: "spectra.api.security.csrf_policy",
        function: security::csrf_policy,
    },
    HostCallSpec {
        name: "spectra.api.security.csrf_allow_origin",
        function: security::csrf_allow_origin,
    },
    HostCallSpec {
        name: "spectra.api.security.csrf_origin_count",
        function: security::csrf_origin_count,
    },
    HostCallSpec {
        name: "spectra.api.security.csrf_middleware",
        function: security::csrf_middleware,
    },
    HostCallSpec {
        name: "spectra.api.security.ssrf_policy",
        function: security::ssrf_policy,
    },
    HostCallSpec {
        name: "spectra.api.security.ssrf_allow_private_networks",
        function: security::ssrf_allow_private_networks,
    },
    HostCallSpec {
        name: "spectra.api.security.ssrf_allows",
        function: security::ssrf_allows,
    },
    HostCallSpec {
        name: "spectra.api.middleware.chain",
        function: middleware::chain,
    },
    HostCallSpec {
        name: "spectra.api.middleware.chain_new",
        function: middleware::chain_new,
    },
    HostCallSpec {
        name: "spectra.api.middleware.chain_len",
        function: middleware::chain_len,
    },
    HostCallSpec {
        name: "spectra.api.middleware.register_sync",
        function: middleware::register_sync,
    },
    HostCallSpec {
        name: "spectra.api.middleware.register_sync_short_circuit",
        function: middleware::register_sync_short_circuit,
    },
    HostCallSpec {
        name: "spectra.api.middleware.register_async",
        function: middleware::register_async,
    },
    HostCallSpec {
        name: "spectra.api.middleware.register_async_short_circuit",
        function: middleware::register_async_short_circuit,
    },
    HostCallSpec {
        name: "spectra.api.middleware.register_logging",
        function: middleware::register_logging,
    },
    HostCallSpec {
        name: "spectra.api.middleware.logging_len",
        function: middleware::logging_len,
    },
    HostCallSpec {
        name: "spectra.api.middleware.logging_line",
        function: middleware::logging_line,
    },
    HostCallSpec {
        name: "spectra.api.middleware.logging_request_id",
        function: middleware::logging_request_id,
    },
    HostCallSpec {
        name: "spectra.api.middleware.register_rate_limit",
        function: middleware::register_rate_limit,
    },
    HostCallSpec {
        name: "spectra.api.middleware.rate_limit_update",
        function: middleware::rate_limit_update,
    },
    HostCallSpec {
        name: "spectra.api.middleware.register_api_key",
        function: middleware::register_api_key,
    },
    HostCallSpec {
        name: "spectra.api.middleware.api_key_add",
        function: middleware::api_key_add,
    },
    HostCallSpec {
        name: "spectra.api.middleware.api_key_revoke",
        function: middleware::api_key_revoke,
    },
    HostCallSpec {
        name: "spectra.api.middleware.register_security_headers",
        function: middleware::register_security_headers,
    },
    HostCallSpec {
        name: "spectra.api.middleware.register_compression",
        function: middleware::register_compression,
    },
    HostCallSpec {
        name: "spectra.api.middleware.security_headers_configure",
        function: middleware::security_headers_configure,
    },
    HostCallSpec {
        name: "spectra.api.middleware.security_headers_route",
        function: middleware::security_headers_route,
    },
    HostCallSpec {
        name: "spectra.api.middleware.use_sync",
        function: middleware::use_sync,
    },
    HostCallSpec {
        name: "spectra.api.middleware.use_async",
        function: middleware::use_async,
    },
    HostCallSpec {
        name: "spectra.api.middleware.execute_sync",
        function: middleware::execute_sync,
    },
    HostCallSpec {
        name: "spectra.api.middleware.execute_async",
        function: middleware::execute_async,
    },
    HostCallSpec {
        name: "spectra.api.middleware.last_trace",
        function: middleware::last_trace,
    },
    HostCallSpec {
        name: "spectra.api.middleware.trace_len",
        function: middleware::trace_len,
    },
    HostCallSpec {
        name: "spectra.api.middleware.trace_event",
        function: middleware::trace_event,
    },
    HostCallSpec {
        name: "spectra.api.middleware.trace_short_circuited",
        function: middleware::trace_short_circuited,
    },
    HostCallSpec {
        name: "spectra.api.validation.schema",
        function: validation::schema,
    },
    HostCallSpec {
        name: "spectra.api.validation.field",
        function: validation::field,
    },
    HostCallSpec {
        name: "spectra.api.validation.min_length",
        function: validation::min_length,
    },
    HostCallSpec {
        name: "spectra.api.validation.max_length",
        function: validation::max_length,
    },
    HostCallSpec {
        name: "spectra.api.validation.range",
        function: validation::range,
    },
    HostCallSpec {
        name: "spectra.api.validation.regex",
        function: validation::regex,
    },
    HostCallSpec {
        name: "spectra.api.validation.validate_json",
        function: validation::validate_json,
    },
    HostCallSpec {
        name: "spectra.api.validation.validate_form",
        function: validation::validate_form,
    },
    HostCallSpec {
        name: "spectra.api.validation.result_ok",
        function: validation::result_ok,
    },
    HostCallSpec {
        name: "spectra.api.validation.result_count",
        function: validation::result_count,
    },
    HostCallSpec {
        name: "spectra.api.validation.result_field",
        function: validation::result_field,
    },
    HostCallSpec {
        name: "spectra.api.validation.result_code",
        function: validation::result_code,
    },
    HostCallSpec {
        name: "spectra.api.validation.result_message",
        function: validation::result_message,
    },
    HostCallSpec {
        name: "spectra.api.validation.result_problem_json",
        function: validation::result_problem_json,
    },
    HostCallSpec {
        name: "spectra.api.validation.result_response",
        function: validation::result_response,
    },
    HostCallSpec {
        name: "spectra.api.validation.error_code",
        function: validation::error_code,
    },
    HostCallSpec {
        name: "spectra.api.validation.error_message",
        function: validation::error_message,
    },
    HostCallSpec {
        name: "spectra.api.errors.last_code",
        function: errors::last_code,
    },
    HostCallSpec {
        name: "spectra.api.errors.last_message",
        function: errors::last_message,
    },
    HostCallSpec {
        name: "spectra.api.errors.new",
        function: errors::new,
    },
    HostCallSpec {
        name: "spectra.api.errors.internal_error",
        function: errors::internal,
    },
    HostCallSpec {
        name: "spectra.api.errors.status",
        function: errors::status,
    },
    HostCallSpec {
        name: "spectra.api.errors.code",
        function: errors::code,
    },
    HostCallSpec {
        name: "spectra.api.errors.message",
        function: errors::message,
    },
    HostCallSpec {
        name: "spectra.api.errors.response",
        function: errors::response,
    },
    HostCallSpec {
        name: "spectra.api.errors.exception_middleware",
        function: errors::exception_middleware,
    },
    HostCallSpec {
        name: "spectra.api.trace.config_new",
        function: trace::config_new,
    },
    HostCallSpec {
        name: "spectra.api.trace.config_set_sample_rate",
        function: trace::config_set_sample_rate,
    },
    HostCallSpec {
        name: "spectra.api.trace.config_set_batch_size",
        function: trace::config_set_batch_size,
    },
    HostCallSpec {
        name: "spectra.api.trace.config_start",
        function: trace::config_start,
    },
    HostCallSpec {
        name: "spectra.api.trace.config_shutdown",
        function: trace::config_shutdown,
    },
    HostCallSpec {
        name: "spectra.api.trace.span_start",
        function: trace::span_start,
    },
    HostCallSpec {
        name: "spectra.api.trace.span_set_attribute",
        function: trace::span_set_attribute,
    },
    HostCallSpec {
        name: "spectra.api.trace.span_set_attribute_int",
        function: trace::span_set_attribute_int,
    },
    HostCallSpec {
        name: "spectra.api.trace.span_set_attribute_bool",
        function: trace::span_set_attribute_bool,
    },
    HostCallSpec {
        name: "spectra.api.trace.span_set_status",
        function: trace::span_set_status,
    },
    HostCallSpec {
        name: "spectra.api.trace.span_end",
        function: trace::span_end,
    },
    HostCallSpec {
        name: "spectra.api.trace.current",
        function: trace::current,
    },
    HostCallSpec {
        name: "spectra.api.trace.parent",
        function: trace::parent,
    },
    HostCallSpec {
        name: "spectra.api.trace.inject",
        function: trace::inject,
    },
    HostCallSpec {
        name: "spectra.api.trace.extract",
        function: trace::extract,
    },
    HostCallSpec {
        name: "spectra.api.trace.flush",
        function: trace::flush,
    },
    HostCallSpec {
        name: "spectra.api.trace.last_error",
        function: trace::last_error,
    },
    HostCallSpec {
        name: "spectra.api.session.memory_store",
        function: session::memory_store,
    },
    HostCallSpec {
        name: "spectra.api.session.redis_store",
        function: session::redis_store,
    },
    HostCallSpec {
        name: "spectra.api.session.store_kind",
        function: session::store_kind,
    },
    HostCallSpec {
        name: "spectra.api.session.create",
        function: session::create,
    },
    HostCallSpec {
        name: "spectra.api.session.lookup",
        function: session::lookup,
    },
    HostCallSpec {
        name: "spectra.api.session.id",
        function: session::id,
    },
    HostCallSpec {
        name: "spectra.api.session.value",
        function: session::value,
    },
    HostCallSpec {
        name: "spectra.api.session.created_at_ms",
        function: session::created_at_ms,
    },
    HostCallSpec {
        name: "spectra.api.session.expires_at_ms",
        function: session::expires_at_ms,
    },
    HostCallSpec {
        name: "spectra.api.session.is_valid",
        function: session::is_valid,
    },
    HostCallSpec {
        name: "spectra.api.session.revoke",
        function: session::revoke,
    },
    HostCallSpec {
        name: "spectra.api.session.error_code",
        function: session::error_code,
    },
    HostCallSpec {
        name: "spectra.api.session.error_message",
        function: session::error_message,
    },
    HostCallSpec {
        name: "spectra.api.websocket.client_new",
        function: websocket::client_new,
    },
    HostCallSpec {
        name: "spectra.api.websocket.client_set_per_message_deflate",
        function: websocket::client_set_per_message_deflate,
    },
    HostCallSpec {
        name: "spectra.api.websocket.client_set_max_message_bytes",
        function: websocket::client_set_max_message_bytes,
    },
    HostCallSpec {
        name: "spectra.api.websocket.client_set_reconnect",
        function: websocket::client_set_reconnect,
    },
    HostCallSpec {
        name: "spectra.api.websocket.client_allow_private_networks",
        function: websocket::client_allow_private_networks,
    },
    HostCallSpec {
        name: "spectra.api.websocket.client_connect",
        function: websocket::client_connect,
    },
    HostCallSpec {
        name: "spectra.api.websocket.server_new",
        function: websocket::server_new,
    },
    HostCallSpec {
        name: "spectra.api.websocket.server_route",
        function: websocket::server_route,
    },
    HostCallSpec {
        name: "spectra.api.websocket.server_listen",
        function: websocket::server_listen,
    },
    HostCallSpec {
        name: "spectra.api.websocket.server_local_port",
        function: websocket::server_local_port,
    },
    HostCallSpec {
        name: "spectra.api.websocket.server_set_per_message_deflate",
        function: websocket::server_set_per_message_deflate,
    },
    HostCallSpec {
        name: "spectra.api.websocket.server_set_max_message_bytes",
        function: websocket::server_set_max_message_bytes,
    },
    HostCallSpec {
        name: "spectra.api.websocket.server_accept",
        function: websocket::server_accept,
    },
    HostCallSpec {
        name: "spectra.api.websocket.connection_peer_port",
        function: websocket::connection_peer_port,
    },
    HostCallSpec {
        name: "spectra.api.websocket.connection_receive",
        function: websocket::connection_receive,
    },
    HostCallSpec {
        name: "spectra.api.websocket.connection_send_text",
        function: websocket::connection_send_text,
    },
    HostCallSpec {
        name: "spectra.api.websocket.connection_send_binary_base64",
        function: websocket::connection_send_binary_base64,
    },
    HostCallSpec {
        name: "spectra.api.websocket.connection_ping",
        function: websocket::connection_ping,
    },
    HostCallSpec {
        name: "spectra.api.websocket.connection_close",
        function: websocket::connection_close,
    },
    HostCallSpec {
        name: "spectra.api.websocket.message_kind",
        function: websocket::message_kind,
    },
    HostCallSpec {
        name: "spectra.api.websocket.message_len",
        function: websocket::message_len,
    },
    HostCallSpec {
        name: "spectra.api.websocket.message_text",
        function: websocket::message_text,
    },
    HostCallSpec {
        name: "spectra.api.websocket.message_base64",
        function: websocket::message_base64,
    },
    HostCallSpec {
        name: "spectra.api.websocket.message_release",
        function: websocket::message_release,
    },
    HostCallSpec {
        name: "spectra.api.sse.server_new",
        function: sse::server_new,
    },
    HostCallSpec {
        name: "spectra.api.sse.server_response",
        function: sse::server_response,
    },
    HostCallSpec {
        name: "spectra.api.sse.server_listen",
        function: sse::server_listen,
    },
    HostCallSpec {
        name: "spectra.api.sse.server_local_port",
        function: sse::server_local_port,
    },
    HostCallSpec {
        name: "spectra.api.sse.server_set_heartbeat_interval",
        function: sse::server_set_heartbeat_interval,
    },
    HostCallSpec {
        name: "spectra.api.sse.server_set_replay_capacity",
        function: sse::server_set_replay_capacity,
    },
    HostCallSpec {
        name: "spectra.api.sse.server_set_max_event_bytes",
        function: sse::server_set_max_event_bytes,
    },
    HostCallSpec {
        name: "spectra.api.sse.server_accept",
        function: sse::server_accept,
    },
    HostCallSpec {
        name: "spectra.api.sse.server_publish",
        function: sse::server_publish,
    },
    HostCallSpec {
        name: "spectra.api.sse.event_new",
        function: sse::event_new,
    },
    HostCallSpec {
        name: "spectra.api.sse.event_id",
        function: sse::event_id,
    },
    HostCallSpec {
        name: "spectra.api.sse.event_type",
        function: sse::event_type,
    },
    HostCallSpec {
        name: "spectra.api.sse.event_data",
        function: sse::event_data,
    },
    HostCallSpec {
        name: "spectra.api.sse.event_retry_ms",
        function: sse::event_retry_ms,
    },
    HostCallSpec {
        name: "spectra.api.sse.event_release",
        function: sse::event_release,
    },
    HostCallSpec {
        name: "spectra.api.sse.connection_peer_port",
        function: sse::connection_peer_port,
    },
    HostCallSpec {
        name: "spectra.api.sse.connection_last_event_id",
        function: sse::connection_last_event_id,
    },
    HostCallSpec {
        name: "spectra.api.sse.connection_send",
        function: sse::connection_send,
    },
    HostCallSpec {
        name: "spectra.api.sse.connection_heartbeat",
        function: sse::connection_heartbeat,
    },
    HostCallSpec {
        name: "spectra.api.sse.connection_close",
        function: sse::connection_close,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.open",
        function: db::sqlite_open,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.close",
        function: db::sqlite_close,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.prepare",
        function: db::sqlite_prepare,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.execute_async",
        function: db::sqlite_execute_async,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.bind_null",
        function: db::sqlite_bind_null,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.bind_int",
        function: db::sqlite_bind_int,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.bind_float",
        function: db::sqlite_bind_float,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.bind_text",
        function: db::sqlite_bind_text,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.bind_blob",
        function: db::sqlite_bind_blob,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.step",
        function: db::sqlite_step,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.column_count",
        function: db::sqlite_column_count,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.column_type",
        function: db::sqlite_column_type,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.column_int",
        function: db::sqlite_column_int,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.column_float",
        function: db::sqlite_column_float,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.column_text",
        function: db::sqlite_column_text,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.reset",
        function: db::sqlite_reset,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.finalize",
        function: db::sqlite_finalize,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.begin",
        function: db::sqlite_begin,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.commit",
        function: db::sqlite_commit,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.rollback",
        function: db::sqlite_rollback,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.last_error_code",
        function: db::sqlite_last_error_code,
    },
    HostCallSpec {
        name: "spectra.api.db.sqlite.last_error_message",
        function: db::sqlite_last_error_message,
    },
    HostCallSpec { name: "spectra.api.db.postgres.open", function: db::postgres_open },
    HostCallSpec { name: "spectra.api.db.postgres.close", function: db::postgres_close },
    HostCallSpec { name: "spectra.api.db.postgres.prepare", function: db::postgres_prepare },
    HostCallSpec { name: "spectra.api.db.postgres.bind_null", function: db::postgres_bind_null },
    HostCallSpec { name: "spectra.api.db.postgres.bind_int", function: db::postgres_bind_int },
    HostCallSpec { name: "spectra.api.db.postgres.bind_float", function: db::postgres_bind_float },
    HostCallSpec { name: "spectra.api.db.postgres.bind_text", function: db::postgres_bind_text },
    HostCallSpec { name: "spectra.api.db.postgres.step", function: db::postgres_step },
    HostCallSpec { name: "spectra.api.db.postgres.column_count", function: db::postgres_column_count },
    HostCallSpec { name: "spectra.api.db.postgres.column_type", function: db::postgres_column_type },
    HostCallSpec { name: "spectra.api.db.postgres.column_int", function: db::postgres_column_int },
    HostCallSpec { name: "spectra.api.db.postgres.column_text", function: db::postgres_column_text },
    HostCallSpec { name: "spectra.api.db.postgres.reset", function: db::postgres_reset },
    HostCallSpec { name: "spectra.api.db.postgres.finalize", function: db::postgres_finalize },
    HostCallSpec { name: "spectra.api.db.postgres.begin", function: db::postgres_begin },
    HostCallSpec { name: "spectra.api.db.postgres.commit", function: db::postgres_commit },
    HostCallSpec { name: "spectra.api.db.postgres.rollback", function: db::postgres_rollback },
    HostCallSpec { name: "spectra.api.db.postgres.execute_async", function: db::postgres_execute_async },
    HostCallSpec { name: "spectra.api.db.postgres.step_async", function: db::postgres_step_async },
    HostCallSpec { name: "spectra.api.db.postgres.savepoint", function: db::postgres_savepoint },
    HostCallSpec { name: "spectra.api.db.postgres.rollback_to", function: db::postgres_rollback_to },
    HostCallSpec { name: "spectra.api.db.postgres.release_savepoint", function: db::postgres_release_savepoint },
    HostCallSpec { name: "spectra.api.db.postgres.copy_in_text_async", function: db::postgres_copy_in_text_async },
    HostCallSpec { name: "spectra.api.db.postgres.copy_out_text_async", function: db::postgres_copy_out_text_async },
    HostCallSpec { name: "spectra.api.db.postgres.listen", function: db::postgres_listen },
    HostCallSpec { name: "spectra.api.db.postgres.notify_async", function: db::postgres_notify_async },
    HostCallSpec { name: "spectra.api.db.postgres.notification_next_async", function: db::postgres_notification_next_async },
    HostCallSpec { name: "spectra.api.db.postgres.notification_channel", function: db::postgres_notification_channel },
    HostCallSpec { name: "spectra.api.db.postgres.notification_payload", function: db::postgres_notification_payload },
    HostCallSpec { name: "spectra.api.db.postgres.notification_process_id", function: db::postgres_notification_process_id },
    HostCallSpec { name: "spectra.api.db.postgres.notification_free", function: db::postgres_notification_free },
    HostCallSpec { name: "spectra.api.db.postgres.notification_close", function: db::postgres_notification_close },
    HostCallSpec { name: "spectra.api.db.postgres.last_error_code", function: db::sqlite_last_error_code },
    HostCallSpec { name: "spectra.api.db.postgres.last_error_message", function: db::sqlite_last_error_message },
    HostCallSpec { name: "spectra.api.db.redis.open", function: db::redis_open },
    HostCallSpec { name: "spectra.api.db.redis.close", function: db::redis_close },
    HostCallSpec { name: "spectra.api.db.redis.get", function: db::redis_get },
    HostCallSpec { name: "spectra.api.db.redis.set", function: db::redis_set },
    HostCallSpec { name: "spectra.api.db.redis.delete", function: db::redis_delete },
    HostCallSpec { name: "spectra.api.db.redis.expire", function: db::redis_expire },
    HostCallSpec { name: "spectra.api.db.redis.incr", function: db::redis_incr },
    HostCallSpec { name: "spectra.api.db.redis.exists", function: db::redis_exists },
];
