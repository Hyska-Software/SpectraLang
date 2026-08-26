#[cfg(test)]
mod tests {
    use super::*;
    use spectra_runtime::ffi::{
        clear_host_functions, lookup_host_function, spectra_rt_manual_clear,
        SpectraHostCallContext, HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_NOT_FOUND,
        HOST_STATUS_SUCCESS,
    };
    use std::collections::HashSet;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::Duration;

    fn test_guard() -> MutexGuard<'static, ()> {
        static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("spectra-api test guard poisoned")
    }

    type ClosureInvoker =
        unsafe extern "C" fn(i64, *const i64, usize, *mut i64) -> i32;

    fn call(name: &str, args: &[SpectraHostValue]) -> (i32, SpectraHostValue) {
        call_with_invoker(name, args, None)
    }

    fn call_with_invoker(
        name: &str,
        args: &[SpectraHostValue],
        invoke_fn: Option<ClosureInvoker>,
    ) -> (i32, SpectraHostValue) {
        let func = lookup_host_function(name).expect("host function registered");
        let mut result = [0_i64];
        let mut ctx = SpectraHostCallContext {
            args: args.as_ptr(),
            arg_len: args.len(),
            results: result.as_mut_ptr(),
            result_len: result.len(),
            invoke_fn,
        };
        let status = func(&mut ctx as *mut _);
        (status, result[0])
    }

    const SYNC_CALLBACK: SpectraHostValue = 0x5359_4e43;
    const ASYNC_CALLBACK: SpectraHostValue = 0x4153_594e;

    fn callback_responses() -> &'static Mutex<(SpectraHostValue, SpectraHostValue)> {
        static RESPONSES: OnceLock<Mutex<(SpectraHostValue, SpectraHostValue)>> = OnceLock::new();
        RESPONSES.get_or_init(|| Mutex::new((0, 0)))
    }

    unsafe extern "C" fn test_callback_invoker(
        closure: SpectraHostValue,
        args: *const i64,
        arg_len: usize,
        result: *mut i64,
    ) -> i32 {
        if arg_len != 1 || args.is_null() || result.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let (sync_response, async_response) = *callback_responses()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match closure {
            SYNC_CALLBACK => {
                *result = sync_response;
                HOST_STATUS_SUCCESS
            }
            ASYNC_CALLBACK => {
                let task = match spectra_runtime::stdlib::spawn_background_task(move || {
                    Ok(async_response)
                }) {
                    Ok(task) => task,
                    Err(status) => return status,
                };
                *result = task;
                HOST_STATUS_SUCCESS
            }
            _ => HOST_STATUS_INVALID_ARGUMENT,
        }
    }

    #[test]
    fn host_call_table_is_unique_and_prefixed() {
        let mut names = HashSet::new();
        for spec in HOST_CALLS {
            assert!(spec.name.starts_with(HOST_PREFIX), "{}", spec.name);
            assert!(names.insert(spec.name), "duplicate {}", spec.name);
        }
        assert_eq!(HOST_CALLS.len(), 444);
        let registered_names: HashSet<_> = HOST_CALLS.iter().map(|spec| spec.name).collect();
        for (name, _) in db::POSTGRES_HOST_CALLS {
            assert!(
                registered_names.contains(name),
                "PostgreSQL host call missing from the canonical API registry: {name}"
            );
        }
    }

    #[test]
    fn register_adds_all_api_host_calls_to_runtime_registry() {
        let _guard = test_guard();
        clear_host_functions();
        let inserted = register();
        assert_eq!(inserted, HOST_CALLS.len());
        for spec in HOST_CALLS {
            assert!(lookup_host_function(spec.name).is_some(), "{}", spec.name);
        }
        let second = register();
        assert_eq!(second, 0);
        clear_host_functions();
    }

    #[test]
    fn registered_version_and_http_functions_execute() {
        let _guard = test_guard();
        clear_host_functions();
        register();
        assert_eq!(
            call("spectra.api.version.major", &[]),
            (HOST_STATUS_SUCCESS, 0)
        );
        assert_eq!(
            call("spectra.api.http.method_allows_body", &[http::METHOD_POST]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call("spectra.api.http.status_is_success", &[201]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call("spectra.api.http.status_class", &[404]),
            (HOST_STATUS_SUCCESS, 4)
        );
        assert_eq!(
            call("spectra.api.errors.last_code", &[]),
            (HOST_STATUS_SUCCESS, errors::ERROR_NONE)
        );
        clear_host_functions();
    }

    #[test]
    fn registered_handle_functions_keep_state() {
        let _guard = test_guard();
        clear_host_functions();
        register();
        let (_, req) = call("spectra.api.http.request_new", &[http::METHOD_GET]);
        assert!(req > 0);
        assert_eq!(
            call("spectra.api.http.request_method", &[req]),
            (HOST_STATUS_SUCCESS, http::METHOD_GET)
        );
        let (_, resp) = call("spectra.api.http.response_new", &[204]);
        assert!(resp > 0);
        assert_eq!(
            call("spectra.api.http.response_status", &[resp]),
            (HOST_STATUS_SUCCESS, 204)
        );
        let (_, server) = call("spectra.api.server.new", &[]);
        assert!(server > 0);
        assert_eq!(
            call("spectra.api.server.listen", &[server, 0]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call("spectra.api.server.local_port", &[server]),
            (HOST_STATUS_SUCCESS, 0)
        );
        assert_eq!(
            call("spectra.api.server.shutdown", &[server]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call("spectra.api.server.state", &[server]),
            (HOST_STATUS_SUCCESS, server::SERVER_STATE_STOPPED)
        );
        assert_eq!(
            call(
                "spectra.api.server.signal",
                &[server, server::SERVER_SIGNAL_SIGINT]
            ),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call("spectra.api.server.stats", &[server, 12]),
            (HOST_STATUS_SUCCESS, 0)
        );
        clear_host_functions();
    }

    #[test]
    fn registered_client_request_returns_real_task_and_response_handle() {
        let _guard = test_guard();
        clear_host_functions();
        spectra_rt_manual_clear();
        register();

        let server = server::HttpServer::start(
            server::ServerConfig::default(),
            std::sync::Arc::new(|request| {
                assert_eq!(request.target, "/client");
                server::ServerResponse::text(200, "client response")
            }),
        )
        .expect("client endpoint starts");
        let port = server.local_addr().port();

        let (_, client) = call("spectra.api.client.new", &[]);
        assert_eq!(
            call("spectra.api.client.timeout_ms", &[client]),
            (HOST_STATUS_SUCCESS, 30_000)
        );
        let (_, ssrf_policy) = call("spectra.api.security.ssrf_policy", &[]);
        let (_, local_policy) = call(
            "spectra.api.security.ssrf_allow_private_networks",
            &[ssrf_policy, 1],
        );
        assert_eq!(
            call(
                "spectra.api.client.set_ssrf_policy",
                &[client, local_policy],
            ),
            (HOST_STATUS_SUCCESS, 1)
        );
        let url = alloc_spectra_string(&format!("http://127.0.0.1:{port}/client"));
        let (request_status, request) = call("spectra.api.http.request", &[http::METHOD_GET, url]);
        assert_eq!(request_status, HOST_STATUS_SUCCESS);

        let (task_status, task) = call("spectra.api.client.request", &[client, request]);
        assert_eq!(task_status, HOST_STATUS_SUCCESS);
        assert!(task > 0, "client request returns a Task handle");

        let mut completed = false;
        for _ in 0..200 {
            let (status, ready) = call("spectra.async.task.poll", &[task]);
            assert_eq!(status, HOST_STATUS_SUCCESS);
            if ready == 1 {
                completed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(completed, "non-blocking client task did not complete");
        let (result_status, response) = call("spectra.async.task.result", &[task]);
        assert_eq!(result_status, HOST_STATUS_SUCCESS);
        assert_eq!(
            call("spectra.api.http.response_status", &[response]),
            (HOST_STATUS_SUCCESS, 200)
        );
        assert_eq!(
            call("spectra.api.http.response_body_len", &[response]),
            (HOST_STATUS_SUCCESS, "client response".len() as i64)
        );

        let mut server = server;
        server.shutdown().expect("client endpoint shutdown");
        clear_host_functions();
        spectra_rt_manual_clear();
    }

    #[test]
    fn r2216_registered_server_lifecycle_serves_router_and_signal_shutdown() {
        let _guard = test_guard();
        clear_host_functions();
        spectra_rt_manual_clear();
        register();

        let (_, router) = call("spectra.api.routing.router_new", &[]);
        let route = call(
            "spectra.api.routing.get",
            &[router, alloc_spectra_string("/hello")],
        );
        assert_eq!(route.0, HOST_STATUS_SUCCESS);
        assert!(route.1 > 0);
        let response = call("spectra.api.handler.text", &[alloc_spectra_string("hello")]);
        assert_eq!(response.0, HOST_STATUS_SUCCESS);
        assert!(response.1 > 0);
        let handler = call("spectra.api.handler.register_sync", &[route.1, response.1]);
        assert_eq!(handler.0, HOST_STATUS_SUCCESS);
        assert!(handler.1 > 0);

        let (_, server) = call("spectra.api.server.new", &[]);
        assert_eq!(
            call("spectra.api.server.listen", &[server, 0]),
            (HOST_STATUS_SUCCESS, 1)
        );
        let serve = call("spectra.api.server.serve", &[server, router]);
        assert_eq!(serve.0, HOST_STATUS_SUCCESS);
        assert!(serve.1 > 0, "serve returns a typed Task handle");
        let (_, port) = call("spectra.api.server.local_port", &[server]);
        assert!(port > 0);

        let mut stream =
            TcpStream::connect(("127.0.0.1", port as u16)).expect("connect served host");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        stream
            .write_all(b"GET /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("write request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        let parsed = http::parse_response(&raw).expect("parse response");
        assert_eq!(parsed.status_code, 200);
        assert_eq!(parsed.body.bytes(), b"hello");

        assert_eq!(
            call(
                "spectra.api.server.signal",
                &[server, server::SERVER_SIGNAL_SIGTERM]
            ),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call("spectra.api.server.state", &[server]),
            (HOST_STATUS_SUCCESS, server::SERVER_STATE_STOPPED)
        );
        assert_eq!(
            call("spectra.api.server.stats", &[server, 10]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call("spectra.api.server.stats", &[server, 12]),
            (HOST_STATUS_SUCCESS, 0)
        );

        clear_host_functions();
        spectra_rt_manual_clear();
    }

    #[test]
    fn routes_export_openapi_emits_valid_spec_for_registered_routes() {
        let _guard = test_guard();
        clear_host_functions();
        spectra_rt_manual_clear();
        register();

        let (_, router) = call("spectra.api.routing.router_new", &[]);
        let get_route = call(
            "spectra.api.routing.get",
            &[router, alloc_spectra_string("/users/{id:int}")],
        );
        assert_eq!(get_route.0, HOST_STATUS_SUCCESS);
        assert!(get_route.1 > 0);
        let post_route = call(
            "spectra.api.routing.post",
            &[router, alloc_spectra_string("/users/{id:int}")],
        );
        assert_eq!(post_route.0, HOST_STATUS_SUCCESS);
        assert!(post_route.1 > 0);

        let (status, spec) = call(
            "spectra.api.routing.routes_export_openapi",
            &[
                router,
                alloc_spectra_string("Users API"),
                alloc_spectra_string("1.0.0"),
            ],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let raw = read_spectra_string(spec).expect("spec string result");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("spec parses as JSON");

        assert_eq!(value["openapi"], "3.1.0");
        assert_eq!(value["info"]["title"], "Users API");
        assert_eq!(value["info"]["version"], "1.0.0");
        let users = &value["paths"]["/users/{id}"];
        assert!(
            users["get"].is_object(),
            "GET operation present for /users/{{id}}"
        );
        assert!(
            users["post"].is_object(),
            "POST operation present for /users/{{id}}"
        );
        let id_param = &users["get"]["parameters"][0];
        assert_eq!(id_param["name"], "id");
        assert_eq!(id_param["in"], "path");
        assert_eq!(id_param["required"], true);
        assert_eq!(id_param["schema"]["type"], "integer");
        assert_eq!(users["get"]["responses"]["200"]["description"], "OK");

        clear_host_functions();
        spectra_rt_manual_clear();
    }

    #[test]
    fn routes_set_request_schema_stores_hint_and_export_embeds_request_body() {
        let _guard = test_guard();
        clear_host_functions();
        spectra_rt_manual_clear();
        register();

        let (_, router) = call("spectra.api.routing.router_new", &[]);
        let get_route = call(
            "spectra.api.routing.get",
            &[router, alloc_spectra_string("/users/{id:int}")],
        );
        assert_eq!(get_route.0, HOST_STATUS_SUCCESS);
        let post_route = call(
            "spectra.api.routing.post",
            &[router, alloc_spectra_string("/users/{id:int}")],
        );
        assert_eq!(post_route.0, HOST_STATUS_SUCCESS);

        // No hint yet: POST must not carry a requestBody.
        let (status, spec) = call(
            "spectra.api.routing.routes_export_openapi",
            &[
                router,
                alloc_spectra_string("Users API"),
                alloc_spectra_string("1.0.0"),
            ],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let value: serde_json::Value =
            serde_json::from_str(&read_spectra_string(spec).expect("spec string"))
                .expect("spec parses as JSON");
        assert!(
            value["paths"]["/users/{id}"]["post"]["requestBody"].is_null(),
            "no requestBody without a schema hint"
        );

        // Store the hint; template may use the registered constraint form.
        let schema = r#"{"type":"object","required":["email"],"properties":{"email":{"type":"string"}}}"#;
        let stored = call(
            "spectra.api.routing.routes_set_request_schema",
            &[
                router,
                3,
                alloc_spectra_string("/users/{id:int}"),
                alloc_spectra_string(schema),
            ],
        );
        assert_eq!(stored.0, HOST_STATUS_SUCCESS);
        assert_eq!(stored.1, 1, "schema hint stored for registered route");

        // Unregistered path: no route matches, reports false.
        let missing = call(
            "spectra.api.routing.routes_set_request_schema",
            &[
                router,
                3,
                alloc_spectra_string("/orders"),
                alloc_spectra_string(schema),
            ],
        );
        assert_eq!(missing.0, HOST_STATUS_SUCCESS);
        assert_eq!(missing.1, 0);

        let (status, spec) = call(
            "spectra.api.routing.routes_export_openapi",
            &[
                router,
                alloc_spectra_string("Users API"),
                alloc_spectra_string("1.0.0"),
            ],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let value: serde_json::Value =
            serde_json::from_str(&read_spectra_string(spec).expect("spec string"))
                .expect("spec parses as JSON");
        let post = &value["paths"]["/users/{id}"]["post"];
        assert_eq!(
            post["requestBody"]["content"]["application/json"]["schema"],
            serde_json::json!({
                "type": "object",
                "required": ["email"],
                "properties": { "email": { "type": "string" } },
            }),
            "POST embeds the stored schema verbatim"
        );
        assert!(
            value["paths"]["/users/{id}"]["get"]["requestBody"].is_null(),
            "GET stays requestBody-free even with a POST hint"
        );

        clear_host_functions();
        spectra_rt_manual_clear();
    }

    #[test]
    fn routes_set_request_schema_rejects_invalid_json_with_typed_error() {
        let _guard = test_guard();
        clear_host_functions();
        spectra_rt_manual_clear();
        register();

        let (_, router) = call("spectra.api.routing.router_new", &[]);
        let route = call(
            "spectra.api.routing.post",
            &[router, alloc_spectra_string("/users")],
        );
        assert_eq!(route.0, HOST_STATUS_SUCCESS);

        let bad = call(
            "spectra.api.routing.routes_set_request_schema",
            &[
                router,
                3,
                alloc_spectra_string("/users"),
                alloc_spectra_string("{\"type\": \"object\""),
            ],
        );
        assert_eq!(bad.0, HOST_STATUS_INVALID_ARGUMENT);
        let (_, conflict) = call("spectra.api.routing.last_conflict", &[]);
        let message = read_spectra_string(conflict).expect("error message");
        assert!(
            message.contains("invalid JSON request schema"),
            "typed error surfaced, got: {message}"
        );

        clear_host_functions();
        spectra_rt_manual_clear();
    }

    #[test]
    fn registered_callbacks_execute_sync_and_async_routes_end_to_end() {
        let _guard = test_guard();
        clear_host_functions();
        spectra_rt_manual_clear();
        register();

        let sync_response = call(
            "spectra.api.handler.text",
            &[alloc_spectra_string("sync callback")],
        );
        assert_eq!(sync_response.0, HOST_STATUS_SUCCESS);
        let async_response = call(
            "spectra.api.handler.text",
            &[alloc_spectra_string("async callback")],
        );
        assert_eq!(async_response.0, HOST_STATUS_SUCCESS);
        *callback_responses()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            (sync_response.1, async_response.1);

        let (_, router) = call("spectra.api.routing.router_new", &[]);
        let sync_route = call(
            "spectra.api.routing.get",
            &[router, alloc_spectra_string("/sync")],
        );
        let async_route = call(
            "spectra.api.routing.get",
            &[router, alloc_spectra_string("/async")],
        );
        assert_eq!(sync_route.0, HOST_STATUS_SUCCESS);
        assert_eq!(async_route.0, HOST_STATUS_SUCCESS);

        let sync_handler = call_with_invoker(
            "spectra.api.handler.register_sync_callback",
            &[sync_route.1, SYNC_CALLBACK],
            Some(test_callback_invoker),
        );
        let async_handler = call_with_invoker(
            "spectra.api.handler.register_async_callback",
            &[async_route.1, ASYNC_CALLBACK],
            Some(test_callback_invoker),
        );
        assert_eq!(sync_handler.0, HOST_STATUS_SUCCESS);
        assert_eq!(async_handler.0, HOST_STATUS_SUCCESS);
        assert!(sync_handler.1 > 0);
        assert!(async_handler.1 > 0);

        let (_, server) = call("spectra.api.server.new", &[]);
        assert_eq!(
            call("spectra.api.server.listen", &[server, 0]),
            (HOST_STATUS_SUCCESS, 1)
        );
        let serve = call("spectra.api.server.serve", &[server, router]);
        assert_eq!(serve.0, HOST_STATUS_SUCCESS);
        let (_, port) = call("spectra.api.server.local_port", &[server]);
        assert!(port > 0);

        let fetch = |path: &str| {
            let mut stream =
                TcpStream::connect(("127.0.0.1", port as u16)).expect("connect callback route");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set callback route read timeout");
            let request = format!(
                "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            );
            stream
                .write_all(request.as_bytes())
                .expect("write callback route request");
            let mut raw = Vec::new();
            stream
                .read_to_end(&mut raw)
                .expect("read callback route response");
            http::parse_response(&raw).expect("parse callback route response")
        };

        let sync = fetch("/sync");
        assert_eq!(sync.status_code, 200);
        assert_eq!(sync.body.bytes(), b"sync callback");
        let asynchronous = fetch("/async");
        assert_eq!(asynchronous.status_code, 200);
        assert_eq!(asynchronous.body.bytes(), b"async callback");

        assert_eq!(
            call(
                "spectra.api.server.signal",
                &[server, server::SERVER_SIGNAL_SIGTERM]
            ),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call("spectra.api.server.stats", &[server, 2]),
            (HOST_STATUS_SUCCESS, 2)
        );

        *callback_responses()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = (0, 0);
        clear_host_functions();
        spectra_rt_manual_clear();
    }

    #[test]
    fn string_based_registered_functions_accept_runtime_strings() {
        let _guard = test_guard();
        clear_host_functions();
        register();
        let name = alloc_spectra_string("Content-Type");
        let value = alloc_spectra_string("application/json");
        assert_eq!(
            call("spectra.api.http.header_name_is_valid", &[name]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call("spectra.api.http.header_value_is_valid", &[value]),
            (HOST_STATUS_SUCCESS, 1)
        );
        let invalid = alloc_spectra_string("bad header");
        assert_eq!(
            call("spectra.api.http.header_name_is_valid", &[invalid]),
            (HOST_STATUS_SUCCESS, 0)
        );
        spectra_rt_manual_clear();
        clear_host_functions();
    }

    #[test]
    fn missing_host_call_still_reports_runtime_not_found() {
        let _guard = test_guard();
        clear_host_functions();
        let mut out = [0_i64];
        let name = "spectra.api.http.missing";
        let status = spectra_runtime::ffi::spectra_rt_host_invoke(
            name.as_ptr(),
            name.len(),
            std::ptr::null(),
            0,
            out.as_mut_ptr(),
            1,
        );
        assert_eq!(status, HOST_STATUS_NOT_FOUND);
    }
}
