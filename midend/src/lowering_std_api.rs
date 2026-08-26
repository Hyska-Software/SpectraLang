use super::*;

pub(crate) fn lookup_std_api_host_function(module: &str, function: &str) -> Option<HostFunctionDescriptor> {
    match (module, function) {
        ("http", "method_name") => Some(host_string("spectra.api.http.method_name")),
        ("http", "method_allows_body") => Some(host_int("spectra.api.http.method_allows_body")),
        ("http", "method_is_safe") => Some(host_int("spectra.api.http.method_is_safe")),
        ("http", "method_get") => Some(host_int("spectra.api.http.method_get")),
        ("http", "method_head") => Some(host_int("spectra.api.http.method_head")),
        ("http", "method_post") => Some(host_int("spectra.api.http.method_post")),
        ("http", "method_put") => Some(host_int("spectra.api.http.method_put")),
        ("http", "method_patch") => Some(host_int("spectra.api.http.method_patch")),
        ("http", "method_delete") => Some(host_int("spectra.api.http.method_delete")),
        ("http", "method_options") => Some(host_int("spectra.api.http.method_options")),
        ("http", "status_reason") => Some(host_string("spectra.api.http.status_reason")),
        ("http", "status_class") => Some(host_int("spectra.api.http.status_class")),
        ("http", "status_is_success") => Some(host_int("spectra.api.http.status_is_success")),
        ("http", "status_continue") => Some(host_int("spectra.api.http.status_continue")),
        ("http", "status_switching_protocols") => {
            Some(host_int("spectra.api.http.status_switching_protocols"))
        }
        ("http", "status_ok") => Some(host_int("spectra.api.http.status_ok")),
        ("http", "status_created") => Some(host_int("spectra.api.http.status_created")),
        ("http", "status_accepted") => Some(host_int("spectra.api.http.status_accepted")),
        ("http", "status_no_content") => Some(host_int("spectra.api.http.status_no_content")),
        ("http", "status_moved_permanently") => {
            Some(host_int("spectra.api.http.status_moved_permanently"))
        }
        ("http", "status_found") => Some(host_int("spectra.api.http.status_found")),
        ("http", "status_not_modified") => Some(host_int("spectra.api.http.status_not_modified")),
        ("http", "status_bad_request") => Some(host_int("spectra.api.http.status_bad_request")),
        ("http", "status_unauthorized") => Some(host_int("spectra.api.http.status_unauthorized")),
        ("http", "status_forbidden") => Some(host_int("spectra.api.http.status_forbidden")),
        ("http", "status_not_found") => Some(host_int("spectra.api.http.status_not_found")),
        ("http", "status_method_not_allowed") => {
            Some(host_int("spectra.api.http.status_method_not_allowed"))
        }
        ("http", "status_conflict") => Some(host_int("spectra.api.http.status_conflict")),
        ("http", "status_unsupported_media_type") => {
            Some(host_int("spectra.api.http.status_unsupported_media_type"))
        }
        ("http", "status_unprocessable_content") => {
            Some(host_int("spectra.api.http.status_unprocessable_content"))
        }
        ("http", "status_too_many_requests") => {
            Some(host_int("spectra.api.http.status_too_many_requests"))
        }
        ("http", "status_internal_server_error") => {
            Some(host_int("spectra.api.http.status_internal_server_error"))
        }
        ("http", "status_bad_gateway") => Some(host_int("spectra.api.http.status_bad_gateway")),
        ("http", "status_service_unavailable") => {
            Some(host_int("spectra.api.http.status_service_unavailable"))
        }
        ("http", "status_gateway_timeout") => {
            Some(host_int("spectra.api.http.status_gateway_timeout"))
        }
        ("http", "header_name_is_valid") => Some(host_int("spectra.api.http.header_name_is_valid")),
        ("http", "header_value_is_valid") => {
            Some(host_int("spectra.api.http.header_value_is_valid"))
        }
        ("http", "request") => Some(host_int("spectra.api.http.request")),
        ("http", "request_new") => Some(host_int("spectra.api.http.request_new")),
        ("http", "request_method") => Some(host_int("spectra.api.http.request_method")),
        ("http", "request_path") => Some(host_string("spectra.api.http.request_path")),
        ("http", "request_body") => Some(host_string("spectra.api.http.request_body")),
        ("http", "request_header") => Some(host_string("spectra.api.http.request_header")),
        ("http", "request_with_header") => Some(host_int("spectra.api.http.request_with_header")),
        ("http", "request_with_body") => Some(host_int("spectra.api.http.request_with_body")),
        ("http", "request_cookie") => Some(host_string("spectra.api.http.request_cookie")),
        ("http", "response") => Some(host_int("spectra.api.http.response")),
        ("http", "response_new") => Some(host_int("spectra.api.http.response_new")),
        ("http", "response_status") => Some(host_int("spectra.api.http.response_status")),
        ("http", "response_header") => Some(host_string("spectra.api.http.response_header")),
        ("http", "response_body_len") => Some(host_int("spectra.api.http.response_body_len")),
        ("http", "header") => Some(host_int("spectra.api.http.header")),
        ("http", "header_name") => Some(host_string("spectra.api.http.header_name")),
        ("http", "header_value") => Some(host_string("spectra.api.http.header_value")),
        ("http", "cookie") => Some(host_int("spectra.api.http.cookie")),
        ("http", "cookie_name") => Some(host_string("spectra.api.http.cookie_name")),
        ("http", "cookie_value") => Some(host_string("spectra.api.http.cookie_value")),
        ("http", "cookie_with_options") => {
            Some(host_int("spectra.api.http.cookie_with_options"))
        }
        ("http", "cookie_path") => Some(host_string("spectra.api.http.cookie_path")),
        ("http", "cookie_domain") => Some(host_string("spectra.api.http.cookie_domain")),
        ("http", "cookie_max_age") => Some(host_int("spectra.api.http.cookie_max_age")),
        ("http", "cookie_secure") => Some(host_bool("spectra.api.http.cookie_secure")),
        ("http", "cookie_http_only") => Some(host_bool("spectra.api.http.cookie_http_only")),
        ("http", "cookie_same_site") => Some(host_int("spectra.api.http.cookie_same_site")),
        ("http", "cookie_header") => Some(host_string("spectra.api.http.cookie_header")),
        ("http", "response_with_cookie") => {
            Some(host_int("spectra.api.http.response_with_cookie"))
        }
        ("http", "cookie_sign") => Some(host_int("spectra.api.http.cookie_sign")),
        ("http", "cookie_verify") => Some(host_bool("spectra.api.http.cookie_verify")),
        ("http", "cookie_is_expired") => Some(host_bool("spectra.api.http.cookie_is_expired")),
        ("http", "cookie_error_code") => Some(host_int("spectra.api.http.cookie_error_code")),
        ("http", "cookie_error_message") => {
            Some(host_string("spectra.api.http.cookie_error_message"))
        }
        ("http", "status") => Some(host_int("spectra.api.http.status")),
        ("json", "validate") => Some(host_bool("spectra.api.json.validate")),
        ("json", "kind") => Some(host_int("spectra.api.json.kind")),
        ("json", "parse") => Some(host_int("spectra.api.json.parse")),
        ("json", "value_kind") => Some(host_int("spectra.api.json.value_kind")),
        ("json", "value_len") => Some(host_int("spectra.api.json.value_len")),
        ("json", "value_get") => Some(host_int("spectra.api.json.value_get")),
        ("json", "value_at") => Some(host_int("spectra.api.json.value_at")),
        ("json", "value_text") => Some(host_string("spectra.api.json.value_text")),
        ("json", "value_number_bits") => Some(host_int("spectra.api.json.value_number_bits")),
        ("json", "value_bool") => Some(host_int("spectra.api.json.value_bool")),
        ("json", "value_free") => Some(host_void("spectra.api.json.value_free")),
        ("json", "stringify") => Some(host_string("spectra.api.json.stringify")),
        ("jwt", "sign") => Some(host_string("spectra.api.jwt.sign")),
        ("jwt", "verify") => Some(host_bool("spectra.api.jwt.verify")),
        ("oauth", "client_new") => Some(host_int("spectra.api.oauth.client_new")),
        ("oauth", "client_set_revocation_url") => {
            Some(host_bool("spectra.api.oauth.client_set_revocation_url"))
        }
        ("oauth", "authorization_url") => Some(host_string("spectra.api.oauth.authorization_url")),
        ("oauth", "exchange_code") => Some(host_int("spectra.api.oauth.exchange_code")),
        ("oauth", "refresh") => Some(host_int("spectra.api.oauth.refresh")),
        ("oauth", "revoke") => Some(host_bool("spectra.api.oauth.revoke")),
        ("oauth", "token_access_token") => {
            Some(host_string("spectra.api.oauth.token_access_token"))
        }
        ("oauth", "token_refresh_token") => {
            Some(host_string("spectra.api.oauth.token_refresh_token"))
        }
        ("oauth", "token_type") => Some(host_string("spectra.api.oauth.token_type")),
        ("oauth", "token_expires_at_ms") => {
            Some(host_int("spectra.api.oauth.token_expires_at_ms"))
        }
        ("oauth", "token_scope") => Some(host_string("spectra.api.oauth.token_scope")),
        ("tls", "config_new") => Some(host_int("spectra.api.tls.config_new")),
        ("tls", "config_mode") => Some(host_int("spectra.api.tls.config_mode")),
        ("tls", "client_config") => Some(host_int("spectra.api.tls.client_config")),
        ("errors", "new") => Some(host_int("spectra.api.errors.new")),
        ("errors", "internal_error") => Some(host_int("spectra.api.errors.internal_error")),
        ("errors", "status") => Some(host_int("spectra.api.errors.status")),
        ("errors", "code") => Some(host_string("spectra.api.errors.code")),
        ("errors", "message") => Some(host_string("spectra.api.errors.message")),
        ("errors", "response") => Some(host_int("spectra.api.errors.response")),
        ("errors", "exception_middleware") => {
            Some(host_int("spectra.api.errors.exception_middleware"))
        }
        ("errors", "last_code") => Some(host_int("spectra.api.errors.last_code")),
        ("errors", "last_message") => Some(host_string("spectra.api.errors.last_message")),
        ("security", "csrf_policy") => Some(host_int("spectra.api.security.csrf_policy")),
        ("security", "csrf_allow_origin") => {
            Some(host_int("spectra.api.security.csrf_allow_origin"))
        }
        ("security", "csrf_origin_count") => {
            Some(host_int("spectra.api.security.csrf_origin_count"))
        }
        ("security", "csrf_middleware") => {
            Some(host_int("spectra.api.security.csrf_middleware"))
        }
        ("security", "ssrf_policy") => Some(host_int("spectra.api.security.ssrf_policy")),
        ("security", "ssrf_allow_private_networks") => {
            Some(host_int("spectra.api.security.ssrf_allow_private_networks"))
        }
        ("security", "ssrf_allows") => Some(host_bool("spectra.api.security.ssrf_allows")),
        ("session", "memory_store") => Some(host_int("spectra.api.session.memory_store")),
        ("session", "redis_store") => Some(host_int("spectra.api.session.redis_store")),
        ("session", "store_kind") => Some(host_string("spectra.api.session.store_kind")),
        ("session", "create") => Some(host_int("spectra.api.session.create")),
        ("session", "lookup") => Some(host_int("spectra.api.session.lookup")),
        ("session", "id") => Some(host_string("spectra.api.session.id")),
        ("session", "value") => Some(host_string("spectra.api.session.value")),
        ("session", "created_at_ms") => {
            Some(host_int("spectra.api.session.created_at_ms"))
        }
        ("session", "expires_at_ms") => {
            Some(host_int("spectra.api.session.expires_at_ms"))
        }
        ("session", "is_valid") => Some(host_bool("spectra.api.session.is_valid")),
        ("session", "revoke") => Some(host_bool("spectra.api.session.revoke")),
        ("session", "error_code") => Some(host_int("spectra.api.session.error_code")),
        ("session", "error_message") => {
            Some(host_string("spectra.api.session.error_message"))
        }
        ("websocket", "client_new") => Some(host_int("spectra.api.websocket.client_new")),
        ("websocket", "client_set_per_message_deflate") => Some(host_bool(
            "spectra.api.websocket.client_set_per_message_deflate",
        )),
        ("websocket", "client_set_max_message_bytes") => Some(host_bool(
            "spectra.api.websocket.client_set_max_message_bytes",
        )),
        ("websocket", "client_set_reconnect") => {
            Some(host_bool("spectra.api.websocket.client_set_reconnect"))
        }
        ("websocket", "client_allow_private_networks") => Some(host_bool(
            "spectra.api.websocket.client_allow_private_networks",
        )),
        ("websocket", "client_connect") => {
            Some(host_task_int("spectra.api.websocket.client_connect"))
        }
        ("websocket", "server_new") => Some(host_int("spectra.api.websocket.server_new")),
        ("websocket", "server_route") => {
            Some(host_bool("spectra.api.websocket.server_route"))
        }
        ("websocket", "server_listen") => {
            Some(host_bool("spectra.api.websocket.server_listen"))
        }
        ("websocket", "server_local_port") => {
            Some(host_int("spectra.api.websocket.server_local_port"))
        }
        ("websocket", "server_set_per_message_deflate") => Some(host_bool(
            "spectra.api.websocket.server_set_per_message_deflate",
        )),
        ("websocket", "server_set_max_message_bytes") => Some(host_bool(
            "spectra.api.websocket.server_set_max_message_bytes",
        )),
        ("websocket", "server_accept") => {
            Some(host_task_int("spectra.api.websocket.server_accept"))
        }
        ("websocket", "connection_peer_port") => {
            Some(host_int("spectra.api.websocket.connection_peer_port"))
        }
        ("websocket", "connection_receive") => Some(host_task_int(
            "spectra.api.websocket.connection_receive",
        )),
        ("websocket", "connection_send_text") => Some(host_task_int(
            "spectra.api.websocket.connection_send_text",
        )),
        ("websocket", "connection_send_binary_base64") => Some(host_task_int(
            "spectra.api.websocket.connection_send_binary_base64",
        )),
        ("websocket", "connection_ping") => {
            Some(host_task_int("spectra.api.websocket.connection_ping"))
        }
        ("websocket", "connection_close") => {
            Some(host_task_int("spectra.api.websocket.connection_close"))
        }
        ("websocket", "message_kind") => {
            Some(host_int("spectra.api.websocket.message_kind"))
        }
        ("websocket", "message_len") => Some(host_int("spectra.api.websocket.message_len")),
        ("websocket", "message_text") => {
            Some(host_string("spectra.api.websocket.message_text"))
        }
        ("websocket", "message_base64") => {
            Some(host_string("spectra.api.websocket.message_base64"))
        }
        ("websocket", "message_release") => {
            Some(host_bool("spectra.api.websocket.message_release"))
        }
        ("sse", "server_new") => Some(host_int("spectra.api.sse.server_new")),
        ("sse", "server_response") => Some(host_int("spectra.api.sse.server_response")),
        ("sse", "server_listen") => Some(host_bool("spectra.api.sse.server_listen")),
        ("sse", "server_local_port") => {
            Some(host_int("spectra.api.sse.server_local_port"))
        }
        ("sse", "server_set_heartbeat_interval") => Some(host_bool(
            "spectra.api.sse.server_set_heartbeat_interval",
        )),
        ("sse", "server_set_replay_capacity") => Some(host_bool(
            "spectra.api.sse.server_set_replay_capacity",
        )),
        ("sse", "server_set_max_event_bytes") => Some(host_bool(
            "spectra.api.sse.server_set_max_event_bytes",
        )),
        ("sse", "server_accept") => {
            Some(host_task_int("spectra.api.sse.server_accept"))
        }
        ("sse", "server_publish") => {
            Some(host_task_int("spectra.api.sse.server_publish"))
        }
        ("sse", "event_new") => Some(host_int("spectra.api.sse.event_new")),
        ("sse", "event_id") => Some(host_string("spectra.api.sse.event_id")),
        ("sse", "event_type") => Some(host_string("spectra.api.sse.event_type")),
        ("sse", "event_data") => Some(host_string("spectra.api.sse.event_data")),
        ("sse", "event_retry_ms") => Some(host_int("spectra.api.sse.event_retry_ms")),
        ("sse", "event_release") => Some(host_bool("spectra.api.sse.event_release")),
        ("sse", "connection_peer_port") => {
            Some(host_int("spectra.api.sse.connection_peer_port"))
        }
        ("sse", "connection_last_event_id") => Some(host_string(
            "spectra.api.sse.connection_last_event_id",
        )),
        ("sse", "connection_send") => {
            Some(host_task_int("spectra.api.sse.connection_send"))
        }
        ("sse", "connection_heartbeat") => Some(host_task_int(
            "spectra.api.sse.connection_heartbeat",
        )),
        ("sse", "connection_close") => {
            Some(host_task_int("spectra.api.sse.connection_close"))
        }
        ("server", "new") => Some(host_int("spectra.api.server.new")),
        ("server", "listen") => Some(host_bool("spectra.api.server.listen")),
        ("server", "serve") => Some(host_task_int("spectra.api.server.serve")),
        ("server", "state") => Some(host_int("spectra.api.server.state")),
        ("server", "shutdown") => Some(host_bool("spectra.api.server.shutdown")),
        ("server", "local_port") => Some(host_int("spectra.api.server.local_port")),
        ("server", "signal") => Some(host_bool("spectra.api.server.signal")),
        ("server", "stats") => Some(host_int("spectra.api.server.stats")),
        ("server", "set_max_body_bytes") => {
            Some(host_bool("spectra.api.server.set_max_body_bytes"))
        }
        ("server", "set_read_timeout") => {
            Some(host_bool("spectra.api.server.set_read_timeout"))
        }
        ("server", "set_idle_timeout") => {
            Some(host_bool("spectra.api.server.set_idle_timeout"))
        }
        ("server", "set_tls_certificate") => {
            Some(host_bool("spectra.api.server.set_tls_certificate"))
        }
        ("server", "tls_local_port") => {
            Some(host_int("spectra.api.server.tls_local_port"))
        }
        ("client", "new") => Some(host_int("spectra.api.client.new")),
        ("client", "request") => Some(host_task_int("spectra.api.client.request")),
        ("client", "timeout_ms") => Some(host_int("spectra.api.client.timeout_ms")),
        ("client", "set_ssrf_policy") => {
            Some(host_bool("spectra.api.client.set_ssrf_policy"))
        }
        ("routing", "router") => Some(host_int("spectra.api.routing.router_new")),
        ("routing", "router_new") => Some(host_int("spectra.api.routing.router_new")),
        ("routing", "route_count") => Some(host_int("spectra.api.routing.route_count")),
        ("routing", "route_id") => Some(host_int("spectra.api.routing.route_id")),
        ("routing", "route_add") => Some(host_int("spectra.api.routing.route_add")),
        ("routing", "get") => Some(host_int("spectra.api.routing.get")),
        ("routing", "post") => Some(host_int("spectra.api.routing.post")),
        ("routing", "put") => Some(host_int("spectra.api.routing.put")),
        ("routing", "patch") => Some(host_int("spectra.api.routing.patch")),
        ("routing", "delete") => Some(host_int("spectra.api.routing.delete")),
        ("routing", "route_match") => Some(host_int("spectra.api.routing.route_match")),
        ("routing", "match_route_id") => Some(host_int("spectra.api.routing.match_route_id")),
        ("routing", "match_param") => Some(host_string("spectra.api.routing.match_param")),
        ("routing", "match_param_int") => Some(host_int("spectra.api.routing.match_param_int")),
        ("routing", "last_conflict") => Some(host_string("spectra.api.routing.last_conflict")),
        ("routing", "routes_export_openapi") => {
            Some(host_string("spectra.api.routing.routes_export_openapi"))
        }
        ("routing", "routes_set_request_schema") => {
            Some(host_bool("spectra.api.routing.routes_set_request_schema"))
        }
        ("query", "type_string") => Some(host_int("spectra.api.query.type_string")),
        ("query", "type_int") => Some(host_int("spectra.api.query.type_int")),
        ("query", "type_bool") => Some(host_int("spectra.api.query.type_bool")),
        ("query", "parse") => Some(host_int("spectra.api.query.parse")),
        ("query", "len") => Some(host_int("spectra.api.query.len")),
        ("query", "has") => Some(host_bool("spectra.api.query.has")),
        ("query", "count") => Some(host_int("spectra.api.query.count")),
        ("query", "first") => Some(host_string("spectra.api.query.first")),
        ("query", "value") => Some(host_string("spectra.api.query.value")),
        ("query", "int") => Some(host_int("spectra.api.query.int")),
        ("query", "bool") => Some(host_bool("spectra.api.query.bool")),
        ("query", "schema") => Some(host_int("spectra.api.query.schema")),
        ("query", "schema_field") => Some(host_int("spectra.api.query.schema_field")),
        ("query", "bind") => Some(host_int("spectra.api.query.bind")),
        ("query", "binding_ok") => Some(host_bool("spectra.api.query.binding_ok")),
        ("query", "binding_error") => Some(host_string("spectra.api.query.binding_error")),
        ("query", "binding_count") => Some(host_int("spectra.api.query.binding_count")),
        ("query", "binding_value") => Some(host_string("spectra.api.query.binding_value")),
        ("query", "binding_int") => Some(host_int("spectra.api.query.binding_int")),
        ("query", "binding_bool") => Some(host_bool("spectra.api.query.binding_bool")),
        ("query", "error_code") => Some(host_int("spectra.api.query.error_code")),
        ("query", "error_message") => Some(host_string("spectra.api.query.error_message")),
        ("form", "type_string") => Some(host_int("spectra.api.form.type_string")),
        ("form", "type_int") => Some(host_int("spectra.api.form.type_int")),
        ("form", "type_bool") => Some(host_int("spectra.api.form.type_bool")),
        ("form", "parse") => Some(host_int("spectra.api.form.parse")),
        ("form", "len") => Some(host_int("spectra.api.form.len")),
        ("form", "has") => Some(host_bool("spectra.api.form.has")),
        ("form", "count") => Some(host_int("spectra.api.form.count")),
        ("form", "first") => Some(host_string("spectra.api.form.first")),
        ("form", "value") => Some(host_string("spectra.api.form.value")),
        ("form", "int") => Some(host_int("spectra.api.form.int")),
        ("form", "bool") => Some(host_bool("spectra.api.form.bool")),
        ("form", "schema") => Some(host_int("spectra.api.form.schema")),
        ("form", "schema_field") => Some(host_int("spectra.api.form.schema_field")),
        ("form", "bind") => Some(host_int("spectra.api.form.bind")),
        ("form", "binding_ok") => Some(host_bool("spectra.api.form.binding_ok")),
        ("form", "binding_error") => Some(host_string("spectra.api.form.binding_error")),
        ("form", "binding_count") => Some(host_int("spectra.api.form.binding_count")),
        ("form", "binding_value") => Some(host_string("spectra.api.form.binding_value")),
        ("form", "binding_int") => Some(host_int("spectra.api.form.binding_int")),
        ("form", "binding_bool") => Some(host_bool("spectra.api.form.binding_bool")),
        ("form", "error_code") => Some(host_int("spectra.api.form.error_code")),
        ("form", "error_message") => Some(host_string("spectra.api.form.error_message")),
        ("multipart", "parse") => Some(host_int("spectra.api.multipart.parse")),
        ("multipart", "part_count") => Some(host_int("spectra.api.multipart.part_count")),
        ("multipart", "field_count") => Some(host_int("spectra.api.multipart.field_count")),
        ("multipart", "file_count") => Some(host_int("spectra.api.multipart.file_count")),
        ("multipart", "text") => Some(host_string("spectra.api.multipart.text")),
        ("multipart", "part") => Some(host_int("spectra.api.multipart.part")),
        ("multipart", "part_name") => Some(host_string("spectra.api.multipart.part_name")),
        ("multipart", "part_filename") => Some(host_string("spectra.api.multipart.part_filename")),
        ("multipart", "part_content_type") => {
            Some(host_string("spectra.api.multipart.part_content_type"))
        }
        ("multipart", "part_size") => Some(host_int("spectra.api.multipart.part_size")),
        ("multipart", "part_is_file") => Some(host_bool("spectra.api.multipart.part_is_file")),
        ("multipart", "file_path") => Some(host_string("spectra.api.multipart.file_path")),
        ("multipart", "file_read") => Some(host_string("spectra.api.multipart.file_read")),
        ("multipart", "file_spool_to") => Some(host_bool("spectra.api.multipart.file_spool_to")),
        ("multipart", "error_code") => Some(host_int("spectra.api.multipart.error_code")),
        ("multipart", "error_message") => Some(host_string("spectra.api.multipart.error_message")),
        ("handler", "text") => Some(host_int("spectra.api.handler.text")),
        ("handler", "json") => Some(host_int("spectra.api.handler.json")),
        ("handler", "bytes") => Some(host_int("spectra.api.handler.bytes")),
        ("handler", "status") => Some(host_int("spectra.api.handler.status")),
        ("handler", "with_header") => Some(host_int("spectra.api.handler.with_header")),
        ("handler", "into_response") => Some(host_int("spectra.api.handler.into_response")),
        ("handler", "into_text_response") => {
            Some(host_int("spectra.api.handler.into_text_response"))
        }
        ("handler", "into_status_response") => {
            Some(host_int("spectra.api.handler.into_status_response"))
        }
        ("handler", "error") => Some(host_int("spectra.api.handler.error")),
        ("handler", "error_response") => Some(host_int("spectra.api.handler.error_response")),
        ("handler", "error_code") => Some(host_int("spectra.api.handler.error_code")),
        ("handler", "error_message") => Some(host_string("spectra.api.handler.error_message")),
        ("handler", "last_error_message") => {
            Some(host_string("spectra.api.handler.last_error_message"))
        }
        ("handler", "register_sync") => Some(host_int("spectra.api.handler.register_sync")),
        ("handler", "register_async") => Some(host_int("spectra.api.handler.register_async")),
        ("handler", "register_sync_callback") => {
            Some(host_int("spectra.api.handler.register_sync_callback"))
        }
        ("handler", "register_async_callback") => {
            Some(host_int("spectra.api.handler.register_async_callback"))
        }
        ("handler", "dispatch_sync") => Some(host_int("spectra.api.handler.dispatch_sync")),
        ("handler", "dispatch_async") => Some(host_int("spectra.api.handler.dispatch_async")),
        ("middleware", "chain") => Some(host_int("spectra.api.middleware.chain")),
        ("middleware", "chain_new") => Some(host_int("spectra.api.middleware.chain_new")),
        ("middleware", "chain_len") => Some(host_int("spectra.api.middleware.chain_len")),
        ("middleware", "register_sync") => Some(host_int("spectra.api.middleware.register_sync")),
        ("middleware", "register_sync_short_circuit") => Some(host_int(
            "spectra.api.middleware.register_sync_short_circuit",
        )),
        ("middleware", "register_async") => Some(host_int("spectra.api.middleware.register_async")),
        ("middleware", "register_async_short_circuit") => Some(host_int(
            "spectra.api.middleware.register_async_short_circuit",
        )),
        ("middleware", "register_logging") => {
            Some(host_int("spectra.api.middleware.register_logging"))
        }
        ("middleware", "logging_len") => {
            Some(host_int("spectra.api.middleware.logging_len"))
        }
        ("middleware", "logging_line") => {
            Some(host_string("spectra.api.middleware.logging_line"))
        }
        ("middleware", "logging_request_id") => {
            Some(host_string("spectra.api.middleware.logging_request_id"))
        }
        ("middleware", "register_rate_limit") => {
            Some(host_int("spectra.api.middleware.register_rate_limit"))
        }
        ("middleware", "rate_limit_update") => {
            Some(host_bool("spectra.api.middleware.rate_limit_update"))
        }
        ("middleware", "register_api_key") => {
            Some(host_int("spectra.api.middleware.register_api_key"))
        }
        ("middleware", "api_key_add") => {
            Some(host_bool("spectra.api.middleware.api_key_add"))
        }
        ("middleware", "api_key_revoke") => {
            Some(host_bool("spectra.api.middleware.api_key_revoke"))
        }
        ("middleware", "register_security_headers") => Some(host_int(
            "spectra.api.middleware.register_security_headers",
        )),
        ("middleware", "register_compression") => {
            Some(host_int("spectra.api.middleware.register_compression"))
        }
        ("middleware", "security_headers_configure") => Some(host_bool(
            "spectra.api.middleware.security_headers_configure",
        )),
        ("middleware", "security_headers_route") => Some(host_bool(
            "spectra.api.middleware.security_headers_route",
        )),
        ("middleware", "use_sync") => Some(host_int("spectra.api.middleware.use_sync")),
        ("middleware", "use_async") => Some(host_int("spectra.api.middleware.use_async")),
        ("middleware", "execute_sync") => Some(host_int("spectra.api.middleware.execute_sync")),
        ("middleware", "execute_async") => Some(host_int("spectra.api.middleware.execute_async")),
        ("middleware", "last_trace") => Some(host_int("spectra.api.middleware.last_trace")),
        ("middleware", "trace_len") => Some(host_int("spectra.api.middleware.trace_len")),
        ("middleware", "trace_event") => Some(host_string("spectra.api.middleware.trace_event")),
        ("middleware", "trace_short_circuited") => {
            Some(host_bool("spectra.api.middleware.trace_short_circuited"))
        }
        ("validation", "schema") => Some(host_int("spectra.api.validation.schema")),
        ("validation", "field") => Some(host_int("spectra.api.validation.field")),
        ("validation", "min_length") => Some(host_int("spectra.api.validation.min_length")),
        ("validation", "max_length") => Some(host_int("spectra.api.validation.max_length")),
        ("validation", "range") => Some(host_int("spectra.api.validation.range")),
        ("validation", "regex") => Some(host_int("spectra.api.validation.regex")),
        ("validation", "validate_json") => {
            Some(host_int("spectra.api.validation.validate_json"))
        }
        ("validation", "validate_form") => {
            Some(host_int("spectra.api.validation.validate_form"))
        }
        ("validation", "result_ok") => Some(host_bool("spectra.api.validation.result_ok")),
        ("validation", "result_count") => {
            Some(host_int("spectra.api.validation.result_count"))
        }
        ("validation", "result_field") => {
            Some(host_string("spectra.api.validation.result_field"))
        }
        ("validation", "result_code") => Some(host_string("spectra.api.validation.result_code")),
        ("validation", "result_message") => {
            Some(host_string("spectra.api.validation.result_message"))
        }
        ("validation", "result_problem_json") => {
            Some(host_string("spectra.api.validation.result_problem_json"))
        }
        ("validation", "result_response") => {
            Some(host_int("spectra.api.validation.result_response"))
        }
        ("validation", "error_code") => Some(host_int("spectra.api.validation.error_code")),
        ("validation", "error_message") => {
            Some(host_string("spectra.api.validation.error_message"))
        }
        ("trace", "config_new") => Some(host_int("spectra.api.trace.config_new")),
        ("trace", "config_set_sample_rate") => Some(host_bool("spectra.api.trace.config_set_sample_rate")),
        ("trace", "config_set_batch_size") => Some(host_bool("spectra.api.trace.config_set_batch_size")),
        ("trace", "config_start") => Some(host_bool("spectra.api.trace.config_start")),
        ("trace", "config_shutdown") => Some(host_bool("spectra.api.trace.config_shutdown")),
        ("trace", "span_start") => Some(host_int("spectra.api.trace.span_start")),
        ("trace", "span_set_attribute") => Some(host_bool("spectra.api.trace.span_set_attribute")),
        ("trace", "span_set_attribute_int") => Some(host_bool("spectra.api.trace.span_set_attribute_int")),
        ("trace", "span_set_attribute_bool") => Some(host_bool("spectra.api.trace.span_set_attribute_bool")),
        ("trace", "span_set_status") => Some(host_bool("spectra.api.trace.span_set_status")),
        ("trace", "span_end") => Some(host_bool("spectra.api.trace.span_end")),
        ("trace", "current") => Some(host_int("spectra.api.trace.current")),
        ("trace", "parent") => Some(host_int("spectra.api.trace.parent")),
        ("trace", "inject") => Some(host_bool("spectra.api.trace.inject")),
        ("trace", "extract") => Some(host_bool("spectra.api.trace.extract")),
        ("trace", "flush") => Some(host_int("spectra.api.trace.flush")),
        ("trace", "last_error") => Some(host_string("spectra.api.trace.last_error")),
        ("health", "startup_complete") => Some(host_bool("spectra.api.health.startup_complete")),
        ("health", "startup_failed") => Some(host_bool("spectra.api.health.startup_failed")),
        ("db.sqlite", "open") => Some(host_int("spectra.api.db.sqlite.open")),
        ("db.sqlite", "close") => Some(host_bool("spectra.api.db.sqlite.close")),
        ("db.sqlite", "prepare") => Some(host_int("spectra.api.db.sqlite.prepare")),
        ("db.sqlite", "execute_async") => Some(host_task_int("spectra.api.db.sqlite.execute_async")),
        ("db.sqlite", "bind_null") => Some(host_bool("spectra.api.db.sqlite.bind_null")),
        ("db.sqlite", "bind_int") => Some(host_bool("spectra.api.db.sqlite.bind_int")),
        ("db.sqlite", "bind_float") => Some(host_bool("spectra.api.db.sqlite.bind_float")),
        ("db.sqlite", "bind_text") => Some(host_bool("spectra.api.db.sqlite.bind_text")),
        ("db.sqlite", "bind_blob") => Some(host_bool("spectra.api.db.sqlite.bind_blob")),
        ("db.sqlite", "step") => Some(host_int("spectra.api.db.sqlite.step")),
        ("db.sqlite", "column_count") => Some(host_int("spectra.api.db.sqlite.column_count")),
        ("db.sqlite", "column_type") => Some(host_int("spectra.api.db.sqlite.column_type")),
        ("db.sqlite", "column_int") => Some(host_int("spectra.api.db.sqlite.column_int")),
        ("db.sqlite", "column_float") => Some(host_float("spectra.api.db.sqlite.column_float")),
        ("db.sqlite", "column_text") => Some(host_string("spectra.api.db.sqlite.column_text")),
        ("db.sqlite", "reset") => Some(host_bool("spectra.api.db.sqlite.reset")),
        ("db.sqlite", "finalize") => Some(host_bool("spectra.api.db.sqlite.finalize")),
        ("db.sqlite", "begin") => Some(host_bool("spectra.api.db.sqlite.begin")),
        ("db.sqlite", "commit") => Some(host_bool("spectra.api.db.sqlite.commit")),
        ("db.sqlite", "rollback") => Some(host_bool("spectra.api.db.sqlite.rollback")),
        ("db.sqlite", "last_error_code") => Some(host_string("spectra.api.db.sqlite.last_error_code")),
        ("db.sqlite", "last_error_message") => Some(host_string("spectra.api.db.sqlite.last_error_message")),
        ("db.postgres", "open") => Some(host_int("spectra.api.db.postgres.open")),
        ("db.postgres", "close") => Some(host_bool("spectra.api.db.postgres.close")),
        ("db.postgres", "prepare") => Some(host_int("spectra.api.db.postgres.prepare")),
        ("db.postgres", "bind_null") => Some(host_bool("spectra.api.db.postgres.bind_null")),
        ("db.postgres", "bind_int") => Some(host_bool("spectra.api.db.postgres.bind_int")),
        ("db.postgres", "bind_float") => Some(host_bool("spectra.api.db.postgres.bind_float")),
        ("db.postgres", "bind_text") => Some(host_bool("spectra.api.db.postgres.bind_text")),
        ("db.postgres", "step") => Some(host_int("spectra.api.db.postgres.step")),
        ("db.postgres", "column_count") => Some(host_int("spectra.api.db.postgres.column_count")),
        ("db.postgres", "column_type") => Some(host_int("spectra.api.db.postgres.column_type")),
        ("db.postgres", "column_int") => Some(host_int("spectra.api.db.postgres.column_int")),
        ("db.postgres", "column_text") => Some(host_string("spectra.api.db.postgres.column_text")),
        ("db.postgres", "reset") => Some(host_bool("spectra.api.db.postgres.reset")),
        ("db.postgres", "finalize") => Some(host_bool("spectra.api.db.postgres.finalize")),
        ("db.postgres", "begin") => Some(host_bool("spectra.api.db.postgres.begin")),
        ("db.postgres", "commit") => Some(host_bool("spectra.api.db.postgres.commit")),
        ("db.postgres", "rollback") => Some(host_bool("spectra.api.db.postgres.rollback")),
        ("db.postgres", "execute_async") => Some(host_task_int("spectra.api.db.postgres.execute_async")),
        ("db.postgres", "step_async") => Some(host_task_int("spectra.api.db.postgres.step_async")),
        ("db.postgres", "savepoint") => Some(host_bool("spectra.api.db.postgres.savepoint")),
        ("db.postgres", "rollback_to") => Some(host_bool("spectra.api.db.postgres.rollback_to")),
        ("db.postgres", "release_savepoint") => Some(host_bool("spectra.api.db.postgres.release_savepoint")),
        ("db.postgres", "copy_in_text_async") => Some(host_task_int("spectra.api.db.postgres.copy_in_text_async")),
        ("db.postgres", "copy_out_text_async") => Some(host_task_string("spectra.api.db.postgres.copy_out_text_async")),
        ("db.postgres", "listen") => Some(host_int("spectra.api.db.postgres.listen")),
        ("db.postgres", "notify_async") => Some(host_task_bool("spectra.api.db.postgres.notify_async")),
        ("db.postgres", "notification_next_async") => Some(host_task_int("spectra.api.db.postgres.notification_next_async")),
        ("db.postgres", "notification_channel") => Some(host_string("spectra.api.db.postgres.notification_channel")),
        ("db.postgres", "notification_payload") => Some(host_string("spectra.api.db.postgres.notification_payload")),
        ("db.postgres", "notification_process_id") => Some(host_int("spectra.api.db.postgres.notification_process_id")),
        ("db.postgres", "notification_free") => Some(host_bool("spectra.api.db.postgres.notification_free")),
        ("db.postgres", "notification_close") => Some(host_bool("spectra.api.db.postgres.notification_close")),
        ("db.postgres", "last_error_code") => Some(host_string("spectra.api.db.postgres.last_error_code")),
        ("db.postgres", "last_error_message") => Some(host_string("spectra.api.db.postgres.last_error_message")),
        ("db.redis", "open") => Some(host_int("spectra.api.db.redis.open")),
        ("db.redis", "open_async") => Some(host_task_int("spectra.api.db.redis.open_async")),
        ("db.redis", "close") => Some(host_bool("spectra.api.db.redis.close")),
        ("db.redis", "close_async") => Some(host_task_bool("spectra.api.db.redis.close_async")),
        ("db.redis", "get") => Some(host_string("spectra.api.db.redis.get")),
        ("db.redis", "get_async") => Some(host_task_string("spectra.api.db.redis.get_async")),
        ("db.redis", "set") => Some(host_bool("spectra.api.db.redis.set")),
        ("db.redis", "set_async") => Some(host_task_bool("spectra.api.db.redis.set_async")),
        ("db.redis", "delete") => Some(host_bool("spectra.api.db.redis.delete")),
        ("db.redis", "delete_async") => Some(host_task_bool("spectra.api.db.redis.delete_async")),
        ("db.redis", "exists") => Some(host_bool("spectra.api.db.redis.exists")),
        ("db.redis", "exists_async") => Some(host_task_bool("spectra.api.db.redis.exists_async")),
        ("db.redis", "incr") => Some(host_int("spectra.api.db.redis.incr")),
        ("db.redis", "incr_async") => Some(host_task_int("spectra.api.db.redis.incr_async")),
        ("db.redis", "expire") => Some(host_bool("spectra.api.db.redis.expire")),
        ("db.redis", "expire_async") => Some(host_task_bool("spectra.api.db.redis.expire_async")),
        ("db.redis", "last_error_code") => Some(host_string("spectra.api.db.redis.last_error_code")),
        ("db.redis", "last_error_message") => Some(host_string("spectra.api.db.redis.last_error_message")),
        ("db.pool", "sqlite_open") => Some(host_int("spectra.api.db.pool.sqlite_open")),
        ("db.pool", "close") => Some(host_bool("spectra.api.db.pool.close")),
        ("db.pool", "with_connection") => {
            Some(host_int("spectra.api.db.pool.with_connection"))
        }
        ("db.migrate", "apply_sqlite") => Some(host_int("spectra.api.db.migrate.apply_sqlite")),
        ("db.migrate", "status_sqlite") => {
            Some(host_string("spectra.api.db.migrate.status_sqlite"))
        }
        ("cors", "policy") => Some(host_int("spectra.api.cors.policy")),
        ("cors", "permissive") => Some(host_int("spectra.api.cors.permissive")),
        ("cors", "allow_origin") => Some(host_int("spectra.api.cors.allow_origin")),
        ("cors", "allow_method") => Some(host_int("spectra.api.cors.allow_method")),
        ("cors", "allow_header") => Some(host_int("spectra.api.cors.allow_header")),
        ("cors", "expose_header") => Some(host_int("spectra.api.cors.expose_header")),
        ("cors", "allow_credentials") => Some(host_int("spectra.api.cors.allow_credentials")),
        ("cors", "max_age") => Some(host_int("spectra.api.cors.max_age")),
        ("cors", "middleware") => Some(host_int("spectra.api.cors.middleware")),
        ("cors", "is_preflight") => Some(host_bool("spectra.api.cors.is_preflight")),
        ("cors", "preflight") => Some(host_int("spectra.api.cors.preflight")),
        ("cors", "apply") => Some(host_int("spectra.api.cors.apply")),
        ("cors", "allowed_origin") => Some(host_string("spectra.api.cors.allowed_origin")),
        _ => None,
    }
}

/// Contract introspection: resolves the registered host-call name the
/// `std.api.<module>.<function>` lowering would target, without exposing the
/// internal descriptor type. Used by cross-crate contract drift tests.
pub fn std_api_host_call_target(module: &str, function: &str) -> Option<&'static str> {
    lookup_std_api_host_function(module, function).map(|d| d.runtime_name)
}

