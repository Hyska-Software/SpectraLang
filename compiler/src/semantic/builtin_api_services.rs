fn make_std_api_handler(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("handler"));
    exports
        .types
        .insert("HandlerHandle".to_string(), public_type(&["route_id"]));
    exports
        .types
        .insert("AsyncHandlerHandle".to_string(), public_type(&["route_id"]));
    exports.types.insert(
        "HandlerError".to_string(),
        public_type(&["status", "message"]),
    );

    let request = api_type("Request");
    let response = api_type("Response");
    let handler_handle = api_type("HandlerHandle");
    let async_handler_handle = api_type("AsyncHandlerHandle");
    let handler_error = api_type("HandlerError");
    let sync_callback = Type::Fn {
        params: vec![request.clone()],
        return_type: Box::new(response.clone()),
    };
    let async_callback = Type::Fn {
        params: vec![request.clone()],
        return_type: Box::new(api_task(response.clone())),
    };

    let functions = [
        ("text", vec![Type::String], response.clone()),
        ("json", vec![Type::String], response.clone()),
        ("bytes", vec![Type::String], response.clone()),
        ("status", vec![Type::Int], response.clone()),
        (
            "with_header",
            vec![response.clone(), Type::String, Type::String],
            response.clone(),
        ),
        ("into_response", vec![response.clone()], response.clone()),
        ("into_text_response", vec![Type::String], response.clone()),
        ("into_status_response", vec![Type::Int], response.clone()),
        (
            "error",
            vec![Type::Int, Type::String],
            handler_error.clone(),
        ),
        (
            "error_response",
            vec![handler_error.clone()],
            response.clone(),
        ),
        ("error_code", vec![handler_error.clone()], Type::Int),
        ("error_message", vec![handler_error], Type::String),
        ("last_error_message", vec![], Type::String),
        (
            "register_sync",
            vec![Type::Int, response.clone()],
            handler_handle.clone(),
        ),
        (
            "register_async",
            vec![Type::Int, response.clone()],
            async_handler_handle.clone(),
        ),
        (
            "register_sync_callback",
            vec![Type::Int, sync_callback],
            handler_handle.clone(),
        ),
        (
            "register_async_callback",
            vec![Type::Int, async_callback],
            async_handler_handle.clone(),
        ),
        (
            "dispatch_sync",
            vec![handler_handle, request.clone()],
            response.clone(),
        ),
        (
            "dispatch_async",
            vec![async_handler_handle, request],
            response,
        ),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }

    exports.traits.insert(
        "IntoResponse".to_string(),
        ExportedTrait {
            visibility: ExportVisibility::Public,
            methods: [(
                "into_response".to_string(),
                exported_trait_method(
                    vec![Type::TypeParameter { name: "T".to_string() }],
                    api_type("Response"),
                    Some(ExportedSelfParamKind::Reference { mutable: false }),
                    false,
                ),
            )]
            .into_iter()
            .collect(),
        },
    );
    exports.traits.insert(
        "Handler".to_string(),
        ExportedTrait {
            visibility: ExportVisibility::Public,
            methods: [(
                "call".to_string(),
                exported_trait_method(
                    vec![
                        Type::TypeParameter { name: "T".to_string() },
                        api_type("Request"),
                    ],
                    api_type("Response"),
                    Some(ExportedSelfParamKind::Reference { mutable: false }),
                    false,
                ),
            )]
            .into_iter()
            .collect(),
        },
    );
    exports.traits.insert(
        "AsyncHandler".to_string(),
        ExportedTrait {
            visibility: ExportVisibility::Public,
            methods: [(
                "call".to_string(),
                exported_trait_method(
                    vec![
                        Type::TypeParameter { name: "T".to_string() },
                        api_type("Request"),
                    ],
                    api_task(api_type("Response")),
                    Some(ExportedSelfParamKind::Reference { mutable: false }),
                    true,
                ),
            )]
            .into_iter()
            .collect(),
        },
    );

    exports
}

fn make_std_api_middleware(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("middleware"));
    exports.types.insert(
        "MiddlewareChain".to_string(),
        public_type(&["middleware_count"]),
    );
    exports
        .types
        .insert("MiddlewareHandle".to_string(), public_type(&["order"]));
    exports
        .types
        .insert("AsyncMiddlewareHandle".to_string(), public_type(&["order"]));
    exports
        .types
        .insert("MiddlewareTrace".to_string(), public_type(&["event_count"]));

    let chain = api_type("MiddlewareChain");
    let middleware = api_type("MiddlewareHandle");
    let async_middleware = api_type("AsyncMiddlewareHandle");
    let trace = api_type("MiddlewareTrace");
    let request = api_type("Request");
    let response = api_type("Response");

    let functions = [
        ("chain", vec![], chain.clone()),
        ("chain_new", vec![], chain.clone()),
        ("chain_len", vec![chain.clone()], Type::Int),
        (
            "register_sync",
            vec![Type::String, Type::String],
            middleware.clone(),
        ),
        (
            "register_sync_short_circuit",
            vec![Type::String, Type::String, response.clone()],
            middleware.clone(),
        ),
        (
            "register_async",
            vec![Type::String, Type::String],
            async_middleware.clone(),
        ),
        (
            "register_async_short_circuit",
            vec![Type::String, Type::String, response.clone()],
            async_middleware.clone(),
        ),
        (
            "register_logging",
            vec![Type::String],
            middleware.clone(),
        ),
        (
            "logging_len",
            vec![middleware.clone()],
            Type::Int,
        ),
        (
            "logging_line",
            vec![middleware.clone(), Type::Int],
            Type::String,
        ),
        (
            "logging_request_id",
            vec![middleware.clone(), Type::Int],
            Type::String,
        ),
        (
            "register_rate_limit",
            vec![Type::String, Type::Int, Type::Int, Type::String, Type::Bool],
            middleware.clone(),
        ),
        (
            "rate_limit_update",
            vec![middleware.clone(), Type::Int, Type::Int],
            Type::Bool,
        ),
        (
            "register_api_key",
            vec![Type::String],
            middleware.clone(),
        ),
        (
            "api_key_add",
            vec![middleware.clone(), Type::String, Type::Int],
            Type::Bool,
        ),
        (
            "api_key_revoke",
            vec![middleware.clone(), Type::String],
            Type::Bool,
        ),
        (
            "register_security_headers",
            vec![],
            middleware.clone(),
        ),
        (
            "register_compression",
            vec![Type::Int],
            middleware.clone(),
        ),
        (
            "security_headers_configure",
            vec![
                middleware.clone(),
                Type::String,
                Type::String,
                Type::Bool,
                Type::Bool,
                Type::Bool,
            ],
            Type::Bool,
        ),
        (
            "security_headers_route",
            vec![middleware.clone(), Type::String, Type::String, Type::String],
            Type::Bool,
        ),
        ("use_sync", vec![chain.clone(), middleware], chain.clone()),
        (
            "use_async",
            vec![chain.clone(), async_middleware],
            chain.clone(),
        ),
        (
            "execute_sync",
            vec![chain.clone(), request.clone(), response.clone()],
            response.clone(),
        ),
        (
            "execute_async",
            vec![chain.clone(), request, response.clone()],
            response.clone(),
        ),
        ("last_trace", vec![], trace.clone()),
        ("trace_len", vec![trace.clone()], Type::Int),
        ("trace_event", vec![trace.clone(), Type::Int], Type::String),
        ("trace_short_circuited", vec![trace], Type::Bool),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }

    exports.traits.insert(
        "Middleware".to_string(),
        ExportedTrait {
            visibility: ExportVisibility::Public,
            methods: [
                (
                    "on_request".to_string(),
                    exported_trait_method(
                        vec![
                            Type::TypeParameter { name: "T".to_string() },
                            api_type("Request"),
                        ],
                        api_type("Request"),
                        Some(ExportedSelfParamKind::Reference { mutable: false }),
                        false,
                    ),
                ),
                (
                    "on_response".to_string(),
                    exported_trait_method(
                        vec![
                            Type::TypeParameter { name: "T".to_string() },
                            api_type("Response"),
                        ],
                        api_type("Response"),
                        Some(ExportedSelfParamKind::Reference { mutable: false }),
                        false,
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        },
    );
    exports.traits.insert(
        "AsyncMiddleware".to_string(),
        ExportedTrait {
            visibility: ExportVisibility::Public,
            methods: [
                (
                    "on_request".to_string(),
                    exported_trait_method(
                        vec![
                            Type::TypeParameter { name: "T".to_string() },
                            api_type("Request"),
                        ],
                        api_task(api_type("Request")),
                        Some(ExportedSelfParamKind::Reference { mutable: false }),
                        true,
                    ),
                ),
                (
                    "on_response".to_string(),
                    exported_trait_method(
                        vec![
                            Type::TypeParameter { name: "T".to_string() },
                            api_type("Response"),
                        ],
                        api_task(api_type("Response")),
                        Some(ExportedSelfParamKind::Reference { mutable: false }),
                        true,
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        },
    );

    exports
}

fn make_std_api_cors(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("cors"));
    exports
        .types
        .insert("CorsPolicy".to_string(), public_type(&["origin_count"]));

    let policy = api_type("CorsPolicy");
    let request = api_type("Request");
    let response = api_type("Response");
    let middleware = api_type("MiddlewareHandle");
    let functions = [
        ("policy", vec![], policy.clone()),
        ("permissive", vec![], policy.clone()),
        (
            "allow_origin",
            vec![policy.clone(), Type::String],
            policy.clone(),
        ),
        (
            "allow_method",
            vec![policy.clone(), Type::Int],
            policy.clone(),
        ),
        (
            "allow_header",
            vec![policy.clone(), Type::String],
            policy.clone(),
        ),
        (
            "expose_header",
            vec![policy.clone(), Type::String],
            policy.clone(),
        ),
        (
            "allow_credentials",
            vec![policy.clone(), Type::Bool],
            policy.clone(),
        ),
        ("max_age", vec![policy.clone(), Type::Int], policy.clone()),
        ("middleware", vec![policy.clone()], middleware),
        ("is_preflight", vec![request.clone()], Type::Bool),
        (
            "preflight",
            vec![policy.clone(), request.clone()],
            response.clone(),
        ),
        (
            "apply",
            vec![policy.clone(), request, response.clone()],
            response,
        ),
        ("allowed_origin", vec![policy, Type::String], Type::String),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }

    exports
}

fn make_std_api_errors(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("errors"));
    exports
        .types
        .insert("ApiError".to_string(), public_type(&["code", "message"]));
    let error = api_type("ApiError");
    let functions = [
        (
            "new",
            vec![Type::Int, Type::String, Type::String],
            error.clone(),
        ),
        (
            "internal_error",
            vec![Type::String, Type::String],
            error.clone(),
        ),
        ("status", vec![error.clone()], Type::Int),
        ("code", vec![error.clone()], Type::String),
        ("message", vec![error.clone()], Type::String),
        ("response", vec![error], api_type("Response")),
        (
            "exception_middleware",
            vec![Type::Int, Type::String, Type::String],
            api_type("MiddlewareHandle"),
        ),
        ("last_code", vec![], Type::Int),
        ("last_message", vec![], Type::String),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_security(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("security"));
    exports
        .types
        .insert("CsrfPolicy".to_string(), public_type(&["origin_count"]));
    exports.types.insert(
        "SsrfPolicy".to_string(),
        public_type(&["allow_private_networks"]),
    );

    let csrf = api_type("CsrfPolicy");
    let ssrf = api_type("SsrfPolicy");
    let middleware = api_type("MiddlewareHandle");
    let functions = [
        ("csrf_policy", vec![], csrf.clone()),
        (
            "csrf_allow_origin",
            vec![csrf.clone(), Type::String],
            csrf.clone(),
        ),
        ("csrf_origin_count", vec![csrf.clone()], Type::Int),
        ("csrf_middleware", vec![csrf], middleware),
        ("ssrf_policy", vec![], ssrf.clone()),
        (
            "ssrf_allow_private_networks",
            vec![ssrf.clone(), Type::Bool],
            ssrf.clone(),
        ),
        ("ssrf_allows", vec![ssrf, Type::String], Type::Bool),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_session(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("session"));
    exports
        .types
        .insert("SessionStore".to_string(), public_type(&["kind"]));
    exports.types.insert(
        "Session".to_string(),
        public_type(&["id", "value", "created_at_ms", "expires_at_ms"]),
    );
    let store = api_type("SessionStore");
    let session = api_type("Session");
    let redis_connection = api_type("RedisConnection");
    let functions = [
        ("memory_store", vec![], store.clone()),
        (
            "redis_store",
            vec![redis_connection, Type::String],
            store.clone(),
        ),
        ("store_kind", vec![store.clone()], Type::String),
        (
            "create",
            vec![store.clone(), Type::String, Type::Int, Type::Int, Type::Bool],
            session.clone(),
        ),
        ("lookup", vec![store.clone(), Type::String], session.clone()),
        ("id", vec![session.clone()], Type::String),
        ("value", vec![session.clone()], Type::String),
        ("created_at_ms", vec![session.clone()], Type::Int),
        ("expires_at_ms", vec![session.clone()], Type::Int),
        ("is_valid", vec![session], Type::Bool),
        ("revoke", vec![store, Type::String], Type::Bool),
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

fn make_std_api_websocket(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("websocket"));
    exports.types.insert(
        "WebSocketServer".to_string(),
        public_type(&["port", "max_message_bytes"]),
    );
    exports
        .types
        .insert("WebSocket".to_string(), public_type(&["peer_port"]));
    exports.types.insert(
        "WebSocketClient".to_string(),
        public_type(&["reconnect_attempts", "reconnect_backoff_ms"]),
    );
    exports.types.insert(
        "WebSocketMessage".to_string(),
        public_type(&["kind", "len"]),
    );
    let server = api_type("WebSocketServer");
    let client = api_type("WebSocketClient");
    let route = api_type("Route");
    let connection = api_type("WebSocket");
    let message = api_type("WebSocketMessage");
    let functions = [
        ("client_new", vec![], client.clone()),
        (
            "client_set_per_message_deflate",
            vec![client.clone(), Type::Bool],
            Type::Bool,
        ),
        (
            "client_set_max_message_bytes",
            vec![client.clone(), Type::Int],
            Type::Bool,
        ),
        (
            "client_set_reconnect",
            vec![client.clone(), Type::Int, Type::Int],
            Type::Bool,
        ),
        (
            "client_allow_private_networks",
            vec![client.clone(), Type::Bool],
            Type::Bool,
        ),
        (
            "client_connect",
            vec![client, Type::String],
            api_task(connection.clone()),
        ),
        ("server_new", vec![], server.clone()),
        (
            "server_route",
            vec![server.clone(), route],
            Type::Bool,
        ),
        (
            "server_listen",
            vec![server.clone(), Type::Int],
            Type::Bool,
        ),
        ("server_local_port", vec![server.clone()], Type::Int),
        (
            "server_set_per_message_deflate",
            vec![server.clone(), Type::Bool],
            Type::Bool,
        ),
        (
            "server_set_max_message_bytes",
            vec![server.clone(), Type::Int],
            Type::Bool,
        ),
        (
            "server_accept",
            vec![server],
            api_task(connection.clone()),
        ),
        ("connection_peer_port", vec![connection.clone()], Type::Int),
        (
            "connection_receive",
            vec![connection.clone()],
            api_task(message.clone()),
        ),
        (
            "connection_send_text",
            vec![connection.clone(), Type::String],
            api_task(Type::Int),
        ),
        (
            "connection_send_binary_base64",
            vec![connection.clone(), Type::String],
            api_task(Type::Int),
        ),
        (
            "connection_ping",
            vec![connection.clone(), Type::String],
            api_task(Type::Int),
        ),
        (
            "connection_close",
            vec![connection, Type::Int, Type::String],
            api_task(Type::Int),
        ),
        ("message_kind", vec![message.clone()], Type::Int),
        ("message_len", vec![message.clone()], Type::Int),
        ("message_text", vec![message.clone()], Type::String),
        ("message_base64", vec![message.clone()], Type::String),
        ("message_release", vec![message], Type::Bool),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_sse(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("sse"));
    exports.types.insert(
        "SseServer".to_string(),
        public_type(&["port", "heartbeat_interval_ms", "replay_capacity"]),
    );
    exports.types.insert(
        "SseConnection".to_string(),
        public_type(&["peer_port", "last_event_id"]),
    );
    exports.types.insert(
        "SseEvent".to_string(),
        public_type(&["id", "event_type", "data", "retry_ms"]),
    );
    let server = api_type("SseServer");
    let connection = api_type("SseConnection");
    let event = api_type("SseEvent");
    let functions = [
        ("server_new", vec![], server.clone()),
        ("server_response", vec![server.clone()], api_type("Response")),
        (
            "server_listen",
            vec![server.clone(), Type::Int],
            Type::Bool,
        ),
        ("server_local_port", vec![server.clone()], Type::Int),
        (
            "server_set_heartbeat_interval",
            vec![server.clone(), Type::Int],
            Type::Bool,
        ),
        (
            "server_set_replay_capacity",
            vec![server.clone(), Type::Int],
            Type::Bool,
        ),
        (
            "server_set_max_event_bytes",
            vec![server.clone(), Type::Int],
            Type::Bool,
        ),
        (
            "server_accept",
            vec![server.clone()],
            api_task(connection.clone()),
        ),
        (
            "server_publish",
            vec![server, event.clone()],
            api_task(Type::Int),
        ),
        (
            "event_new",
            vec![Type::String, Type::String, Type::String, Type::Int],
            event.clone(),
        ),
        ("event_id", vec![event.clone()], Type::String),
        ("event_type", vec![event.clone()], Type::String),
        ("event_data", vec![event.clone()], Type::String),
        ("event_retry_ms", vec![event.clone()], Type::Int),
        ("event_release", vec![event], Type::Bool),
        ("connection_peer_port", vec![connection.clone()], Type::Int),
        (
            "connection_last_event_id",
            vec![connection.clone()],
            Type::String,
        ),
        (
            "connection_send",
            vec![connection.clone(), api_type("SseEvent")],
            api_task(Type::Int),
        ),
        (
            "connection_heartbeat",
            vec![connection.clone()],
            api_task(Type::Int),
        ),
        ("connection_close", vec![connection], api_task(Type::Int)),
    ];
    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }
    exports
}

fn make_std_api_validation(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("validation"));
    exports.types.insert(
        "ValidationSchema".to_string(),
        public_type(&["field_count"]),
    );
    exports.types.insert(
        "ValidationResult".to_string(),
        public_type(&["ok", "issue_count"]),
    );
    let schema = api_type("ValidationSchema");
    let result = api_type("ValidationResult");
    let form = api_type("Form");
    let functions = [
        ("schema", vec![], schema.clone()),
        (
            "field",
            vec![schema.clone(), Type::String, Type::Int, Type::Bool],
            schema.clone(),
        ),
        (
            "min_length",
            vec![schema.clone(), Type::String, Type::Int],
            schema.clone(),
        ),
        (
            "max_length",
            vec![schema.clone(), Type::String, Type::Int],
            schema.clone(),
        ),
        (
            "range",
            vec![schema.clone(), Type::String, Type::Int, Type::Int],
            schema.clone(),
        ),
        (
            "regex",
            vec![schema.clone(), Type::String, Type::String],
            schema.clone(),
        ),
        (
            "validate_json",
            vec![schema.clone(), Type::String],
            result.clone(),
        ),
        (
            "validate_form",
            vec![schema.clone(), form],
            result.clone(),
        ),
        ("result_ok", vec![result.clone()], Type::Bool),
        ("result_count", vec![result.clone()], Type::Int),
        (
            "result_field",
            vec![result.clone(), Type::Int],
            Type::String,
        ),
        (
            "result_code",
            vec![result.clone(), Type::Int],
            Type::String,
        ),
        (
            "result_message",
            vec![result.clone(), Type::Int],
            Type::String,
        ),
        (
            "result_problem_json",
            vec![result.clone()],
            Type::String,
        ),
        (
            "result_response",
            vec![result],
            api_type("Response"),
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

fn make_std_api_trace(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("trace"));
    let config = api_type("TraceConfig");
    let span = api_type("TraceSpan");
    exports.types.insert("TraceConfig".to_string(), public_type(&[]));
    exports.types.insert("TraceSpan".to_string(), public_type(&[]));
    for (name, params, return_type) in [
        ("config_new", vec![Type::String, Type::String], config.clone()),
        ("config_set_sample_rate", vec![config.clone(), Type::Float], Type::Bool),
        ("config_set_batch_size", vec![config.clone(), Type::Int], Type::Bool),
        ("config_start", vec![config.clone()], Type::Bool),
        ("config_shutdown", vec![config.clone()], Type::Bool),
        ("span_start", vec![Type::String, Type::Int], span.clone()),
        ("span_set_attribute", vec![span.clone(), Type::String, Type::String], Type::Bool),
        ("span_set_attribute_int", vec![span.clone(), Type::String, Type::Int], Type::Bool),
        ("span_set_attribute_bool", vec![span.clone(), Type::String, Type::Bool], Type::Bool),
        ("span_set_status", vec![span.clone(), Type::Int], Type::Bool),
        ("span_end", vec![span.clone()], Type::Bool),
        ("current", vec![], span.clone()),
        ("parent", vec![span.clone()], span.clone()),
        ("inject", vec![span.clone()], Type::Bool),
        ("extract", vec![Type::String], Type::Bool),
        ("flush", vec![], Type::Int),
        ("last_error", vec![], Type::String),
    ] { exports.functions.insert(name.to_string(), pub_fn(params, return_type)); }
    exports
}

fn make_std_api_health(prefix: &str) -> ModuleExports {
    let mut exports = api_module(prefix, Some("health"));
    exports.functions.insert("startup_complete".into(), pub_fn(vec![], Type::Bool));
    exports.functions.insert("startup_failed".into(), pub_fn(vec![Type::String], Type::Bool));
    exports
}

fn make_std_api_db_sqlite(prefix: &str) -> ModuleExports {
    let mut exports = api_module(&format!("{prefix}.db.sqlite"), None);
    let connection = api_type("SqliteConnection");
    let statement = api_type("SqliteStatement");
    exports.types.insert("SqliteConnection".to_string(), public_type(&[]));
    exports.types.insert("SqliteStatement".to_string(), public_type(&[]));
    for (name, params, return_type) in [
        ("open", vec![Type::String], connection.clone()),
        ("close", vec![connection.clone()], Type::Bool),
        ("prepare", vec![connection.clone(), Type::String], statement.clone()),
        ("execute_async", vec![connection.clone(), Type::String], api_task(Type::Int)),
        ("bind_null", vec![statement.clone(), Type::Int], Type::Bool),
        ("bind_int", vec![statement.clone(), Type::Int, Type::Int], Type::Bool),
        ("bind_float", vec![statement.clone(), Type::Int, Type::Float], Type::Bool),
        ("bind_text", vec![statement.clone(), Type::Int, Type::String], Type::Bool),
        ("bind_blob", vec![statement.clone(), Type::Int, Type::String], Type::Bool),
        ("step", vec![statement.clone()], Type::Int),
        ("column_count", vec![statement.clone()], Type::Int),
        ("column_type", vec![statement.clone(), Type::Int], Type::Int),
        ("column_int", vec![statement.clone(), Type::Int], Type::Int),
        ("column_float", vec![statement.clone(), Type::Int], Type::Float),
        ("column_text", vec![statement.clone(), Type::Int], Type::String),
        ("reset", vec![statement.clone()], Type::Bool),
        ("finalize", vec![statement.clone()], Type::Bool),
        ("begin", vec![connection.clone()], Type::Bool),
        ("commit", vec![connection.clone()], Type::Bool),
        ("rollback", vec![connection.clone()], Type::Bool),
        ("last_error_code", vec![connection.clone()], Type::String),
        ("last_error_message", vec![connection.clone()], Type::String),
    ] { exports.functions.insert(name.to_string(), pub_fn(params, return_type)); }
    exports
}

fn make_std_api_db_postgres(prefix: &str) -> ModuleExports {
    let mut exports = api_module(&format!("{prefix}.db.postgres"), None);
    let connection = api_type("PostgresConnection");
    let statement = api_type("PostgresStatement");
    let notification_channel = api_type("PostgresNotificationChannel");
    let notification = api_type("PostgresNotification");
    exports.types.insert("PostgresConnection".to_string(), public_type(&[]));
    exports.types.insert("PostgresStatement".to_string(), public_type(&[]));
    exports.types.insert("PostgresNotificationChannel".to_string(), public_type(&[]));
    exports.types.insert("PostgresNotification".to_string(), public_type(&[]));
    for (name, params, return_type) in [
        ("open", vec![Type::String], connection.clone()),
        ("close", vec![connection.clone()], Type::Bool),
        ("prepare", vec![connection.clone(), Type::String], statement.clone()),
        ("bind_null", vec![statement.clone(), Type::Int], Type::Bool),
        ("bind_int", vec![statement.clone(), Type::Int, Type::Int], Type::Bool),
        ("bind_float", vec![statement.clone(), Type::Int, Type::Float], Type::Bool),
        ("bind_text", vec![statement.clone(), Type::Int, Type::String], Type::Bool),
        ("step", vec![statement.clone()], Type::Int),
        ("column_count", vec![statement.clone()], Type::Int),
        ("column_type", vec![statement.clone(), Type::Int], Type::Int),
        ("column_int", vec![statement.clone(), Type::Int], Type::Int),
        ("column_text", vec![statement.clone(), Type::Int], Type::String),
        ("reset", vec![statement.clone()], Type::Bool),
        ("finalize", vec![statement.clone()], Type::Bool),
        ("begin", vec![connection.clone()], Type::Bool),
        ("commit", vec![connection.clone()], Type::Bool),
        ("rollback", vec![connection.clone()], Type::Bool),
        ("execute_async", vec![connection.clone(), Type::String], api_task(Type::Int)),
        ("step_async", vec![statement.clone()], api_task(Type::Int)),
        ("savepoint", vec![connection.clone(), Type::String], Type::Bool),
        ("rollback_to", vec![connection.clone(), Type::String], Type::Bool),
        ("release_savepoint", vec![connection.clone(), Type::String], Type::Bool),
        ("copy_in_text_async", vec![connection.clone(), Type::String, Type::String], api_task(Type::Int)),
        ("copy_out_text_async", vec![connection.clone(), Type::String], api_task(Type::String)),
        ("listen", vec![connection.clone(), Type::String], notification_channel.clone()),
        ("notify_async", vec![connection.clone(), Type::String, Type::String], api_task(Type::Bool)),
        ("notification_next_async", vec![notification_channel.clone(), Type::Int], api_task(notification.clone())),
        ("notification_channel", vec![notification.clone()], Type::String),
        ("notification_payload", vec![notification.clone()], Type::String),
        ("notification_process_id", vec![notification.clone()], Type::Int),
        ("notification_free", vec![notification], Type::Bool),
        ("notification_close", vec![notification_channel], Type::Bool),
        ("last_error_code", vec![connection.clone()], Type::String),
        ("last_error_message", vec![connection.clone()], Type::String),
    ] { exports.functions.insert(name.to_string(), pub_fn(params, return_type)); }
    exports
}

fn make_std_api_db_redis(prefix: &str) -> ModuleExports {
    let mut exports = api_module(&format!("{prefix}.db.redis"), None);
    let connection = api_type("RedisConnection");
    exports.types.insert("RedisConnection".to_string(), public_type(&[]));
    for (name, params, return_type) in [
        ("open", vec![Type::String], connection.clone()),
        ("open_async", vec![Type::String], api_task(Type::Int)),
        ("close", vec![connection.clone()], Type::Bool),
        ("close_async", vec![connection.clone()], api_task(Type::Bool)),
        ("get", vec![connection.clone(), Type::String], Type::String),
        ("get_async", vec![connection.clone(), Type::String], api_task(Type::String)),
        ("set", vec![connection.clone(), Type::String, Type::String], Type::Bool),
        ("set_async", vec![connection.clone(), Type::String, Type::String], api_task(Type::Bool)),
        ("delete", vec![connection.clone(), Type::String], Type::Bool),
        ("delete_async", vec![connection.clone(), Type::String], api_task(Type::Bool)),
        ("exists", vec![connection.clone(), Type::String], Type::Bool),
        ("exists_async", vec![connection.clone(), Type::String], api_task(Type::Bool)),
        ("incr", vec![connection.clone(), Type::String, Type::Int], Type::Int),
        ("incr_async", vec![connection.clone(), Type::String, Type::Int], api_task(Type::Int)),
        ("expire", vec![connection.clone(), Type::String, Type::Int], Type::Bool),
        ("expire_async", vec![connection.clone(), Type::String, Type::Int], api_task(Type::Bool)),
        ("last_error_code", vec![connection.clone()], Type::String),
        ("last_error_message", vec![connection.clone()], Type::String),
    ] { exports.functions.insert(name.to_string(), pub_fn(params, return_type)); }
    exports
}
