//! MCP server: expose a Spectra project's tools to any MCP client (R-3218 T2).
//!
//! `tools/list` is the project's derived `#[agent_tool]` surface: the name,
//! the authored description, the JSON Schema the compiler derived from the
//! payload type, and the standard MCP annotations derived from the
//! compiler-derived effect set ([`taint::is_sink`] classifies a destructive
//! effect). Nothing is re-derived here, so the served surface and the compiled
//! surface cannot drift.
//!
//! `tools/call` executes through the governed dispatch — the same
//! [`crate::act::tool_call`] the model-driven loop uses — inside the serving
//! run and with the run's capability set: the grant check, the tool-call
//! ceiling, the journal step and the taint gate are exactly the ones a local
//! `tool_call` passes. A tool failure is a tool result with `isError`, a
//! run-level refusal (a capability denial, a crossed ceiling) is a JSON-RPC
//! error; neither is ever a panic or a silent success.
//!
//! # The HTTP transport
//!
//! The crate cannot depend on `spectra-api`, so [`serve`] uses the crate's
//! shared HTTP/1.1 listener ([`crate::net`]): POST-only (at any path), one
//! request per connection, `application/json` responses (the JSON response
//! mode of the streamable-HTTP transport). That listener is deliberately small
//! — no TLS, no keep-alive, no back-pressure, no authentication — because the
//! transport, like the client's, belongs to the embedding host; an application
//! that needs those properties serves the same [`handle`] from its own stack
//! (for example `std.api.server`), and should front [`serve`] with it
//! otherwise. What this listener guarantees is that a Spectra project is
//! callable by a third-party MCP client today, and that every call goes
//! through the governed dispatch.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use serde_json::{json, Value};

use crate::error::AgentError;
use crate::mcp::wire;
use crate::net;
use crate::provider::transport::TransportResponse;
use crate::run;
use crate::taint;
use crate::tools;

/// JSON-RPC error codes this adapter uses.
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
/// A run-level refusal: a capability denial or a crossed ceiling. Distinct from
/// a tool failure, which is reported as a tool result with `isError`.
const RUN_REFUSED: i64 = -32001;

/// Authorities this process serves, with the run each one answers for.
fn listeners() -> &'static Mutex<BTreeMap<String, i64>> {
    static LISTENERS: LazyLock<Mutex<BTreeMap<String, i64>>> =
        LazyLock::new(|| Mutex::new(BTreeMap::new()));
    &LISTENERS
}

fn lock() -> MutexGuard<'static, BTreeMap<String, i64>> {
    listeners()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `mcp_handle(run, request)`: answer one JSON-RPC request from the project's
/// registered tools.
///
/// A notification (`initialize`/`notifications/initialized`, no `id`) returns
/// the empty string, which is what the streamable-HTTP transport sends back
/// with `202 Accepted`. A malformed document is a typed failure, because the
/// caller handed the adapter something that is not a request at all.
pub(crate) fn handle(run_handle: i64, request: &str) -> Result<String, AgentError> {
    // The serving run must be live. The listener already answers 410 for a run
    // that has ended; checking here too is what makes the direct entry point
    // and the socket agree, instead of one of them listing the tool surface of
    // a run that no longer exists.
    run::with_run(run_handle, |_| ())?;
    let value: Value = serde_json::from_str(request).map_err(|error| {
        AgentError::Mcp(format!("the MCP request is not a JSON document: {error}"))
    })?;
    if !value.is_object() {
        return Err(AgentError::Mcp(
            "the MCP request must be a JSON-RPC object (batch requests are not supported)"
                .to_string(),
        ));
    }
    let response = dispatch(run_handle, &value);
    if response.is_null() {
        return Ok(String::new());
    }
    Ok(response.to_string())
}

/// `mcp_serve(run, bind)`: start the in-process HTTP listener.
///
/// Returns the bound `host:port`. The listener serves the run's living tools
/// for as long as the run lives; it stops on the first request that arrives
/// after `agent_end`, and always when the process exits.
pub(crate) fn serve(run_handle: i64, bind: &str) -> Result<String, AgentError> {
    run::with_run(run_handle, |_| ())?;
    let handler: net::Handler = Arc::new(move |request: &net::Request| {
        if request.method != "POST" {
            return net::Response::text(
                405,
                "Method Not Allowed",
                format!("the MCP endpoint accepts POST, found {}", request.method),
            );
        }
        if run::with_run(run_handle, |_| ()).is_err() {
            // The serving run has ended: its surface is gone with it.
            return net::Response::text(410, "Gone", "the serving run has ended").stop();
        }
        let body = match handle(run_handle, &request.body) {
            Ok(body) => body,
            Err(error) => error_document(&error),
        };
        net::Response::json(body)
    });
    let authority = net::serve(bind, "spectra-mcp", handler)
        .map_err(|error| AgentError::Mcp(format!("could not start the MCP listener: {error}")))?;
    lock().insert(authority.clone(), run_handle);
    Ok(authority)
}

/// Answers a request against a server this process bound, without a socket.
///
/// Consulted by [`crate::mcp::post`] only when no host transport is installed,
/// and only for a live local listener, so this path can never leave the
/// process and never bypasses a host's own HTTP policy.
pub(crate) fn loopback_post(url: &str, body: &str) -> Option<TransportResponse> {
    let endpoint = wire::Endpoint::parse(url).ok()?;
    let run_handle = *lock().get(&endpoint.authority)?;
    if run::with_run(run_handle, |_| ()).is_err() {
        // A listener whose run has ended is inert; the entry is dropped so it
        // cannot shadow a later listener on the same authority.
        lock().remove(&endpoint.authority);
        return None;
    }
    let response = match handle(run_handle, body) {
        Ok(response) => response,
        Err(error) => error_document(&error),
    };
    Some(TransportResponse {
        status: 200,
        body: response,
    })
}

/// Routes one parsed request to its method.
fn dispatch(run_handle: i64, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    match method {
        "initialize" => wire::ok(
            id,
            json!({
                "protocolVersion": wire::PROTOCOL_VERSION,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {
                    "name": "spectra-agent",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        ),
        // A notification: the transport answers `202 Accepted` with no body.
        "notifications/initialized" => Value::Null,
        "ping" => wire::ok(id, json!({})),
        // A registry that cannot be advertised is a run-level refusal, not an
        // empty list: the client must not read "no tools" when the truth is
        // "this tool's descriptor is broken".
        "tools/list" => match descriptors() {
            Ok(descriptors) => wire::ok(id, json!({"tools": descriptors})),
            Err(error) => wire::failure(id, RUN_REFUSED, &error.message()),
        },
        "tools/call" => call_tool(run_handle, id, &params),
        other => wire::failure(
            id,
            METHOD_NOT_FOUND,
            &format!("the MCP method '{other}' is not served by this project"),
        ),
    }
}

/// The project's derived tool surface, in the registry's deterministic order.
///
/// A tool whose registered schema does not parse is refused by name rather
/// than advertised with a permissive substitute: a client that saw
/// `{"type":"object"}` would send arguments the tool's wrapper rejects, and the
/// broken descriptor would never be reported. The refusal names the tool and
/// the parse error, so the surfacing layer is fixable.
fn descriptors() -> Result<Vec<Value>, AgentError> {
    let mut descriptors = Vec::new();
    for tool in tools::registered() {
        {
            let schema = serde_json::from_str::<Value>(&tool.input_schema).map_err(|error| {
                AgentError::Mcp(format!(
                    "tool '{}' has an input schema that is not JSON ({error}); it cannot be \
                     advertised over MCP until the descriptor is fixed",
                    tool.name
                ))
            })?;
            // The annotations are derived from the compiler's effect set: an
            // effect the catalog classifies as a sink is destructive, and a
            // tool with no effect is read-only.
            let destructive = tool
                .effects
                .iter()
                .any(|effect| taint::is_sink(effect));
            descriptors.push(json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": schema,
                "annotations": {
                    "readOnlyHint": tool.effects.is_empty(),
                    "destructiveHint": destructive,
                },
            }));
        }
    }
    Ok(descriptors)
}

/// One `tools/call`, executed inside the run through the governed dispatch.
fn call_tool(run_handle: i64, id: Value, params: &Value) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return wire::failure(id, INVALID_PARAMS, "tools/call needs a tool 'name'");
    };
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(value @ Value::Object(_)) => value.clone(),
        Some(_) => {
            return wire::failure(id, INVALID_PARAMS, "tools/call 'arguments' must be an object")
        }
    };
    let arguments = arguments.to_string();

    // The same entry point the model-driven loop uses: the run's grant is
    // enforced, the call is charged against its ceiling, and the invocation is
    // journaled. The run is entered on this thread's chain so the tool body's
    // own host calls pass the policy and taint seams.
    let outcome = run::in_run_scope(run_handle, || {
        crate::act::tool_call(run_handle, name, &arguments)
    });
    match outcome {
        Ok(result) => wire::ok(
            id,
            json!({
                "content": [{"type": "text", "text": result}],
                "isError": false,
            }),
        ),
        Err(error @ (AgentError::UnknownTool(_) | AgentError::ToolFailed(_))) => {
            // A tool-level failure is a tool result, so an MCP client can feed
            // it back to its model instead of treating the server as broken.
            // The stable typed prefix is kept, so a client can branch on it.
            wire::ok(
                id,
                json!({
                    "content": [{"type": "text", "text": format!("error: {}", error.message())}],
                    "isError": true,
                }),
            )
        }
        Err(error) => wire::failure(id, RUN_REFUSED, &error.message()),
    }
}

/// A JSON-RPC error document for a request that never reached a method.
fn error_document(error: &AgentError) -> String {
    wire::failure(Value::Null, INVALID_REQUEST, &error.message()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    use crate::provider::transport::{
        clear_http_transport, set_http_transport, HttpTransport, TransportResponse,
    };
    use crate::spec::AgentSpec;

    /// A wrapper with the ADR 0019 ABI: it doubles `n`.
    extern "C" fn doubling_tool(_run: i64, args: i64, out: i64) -> i64 {
        let arguments = crate::abi::read_string_arg(args).unwrap_or_default();
        let n = serde_json::from_str::<Value>(&arguments)
            .ok()
            .and_then(|value| value.get("n").and_then(Value::as_i64))
            .unwrap_or(0);
        let pointer = unsafe { crate::abi::alloc_string(&(n * 2).to_string()) };
        unsafe { *(out as *mut i64) = pointer };
        0
    }

    /// A real HTTP client over TCP, so the round trip in these tests crosses a
    /// socket exactly as a remote client would.
    struct TcpTransport;

    impl HttpTransport for TcpTransport {
        fn post_json(
            &self,
            url: &str,
            _headers: &[(String, String)],
            body: &str,
        ) -> Result<TransportResponse, String> {
            let endpoint = wire::Endpoint::parse(url).map_err(|error| error.message())?;
            let mut stream = TcpStream::connect(&endpoint.authority).map_err(|e| e.to_string())?;
            let request = format!(
                "POST /mcp HTTP/1.1\r\nhost: {}\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                endpoint.authority,
                body.len()
            );
            stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
            let mut raw = String::new();
            stream.read_to_string(&mut raw).map_err(|e| e.to_string())?;
            let (status, body) = parse_http_response(&raw)?;
            Ok(TransportResponse { status, body })
        }
    }

    /// A raw third-party client: one POST, then the response.
    fn third_party_post(authority: &str, body: &str) -> (i64, String) {
        let mut stream = TcpStream::connect(authority).expect("connect");
        let request = format!(
            "POST /mcp HTTP/1.1\r\nhost: {authority}\r\ncontent-type: application/json\r\n\
             content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).expect("write");
        let mut raw = String::new();
        stream.read_to_string(&mut raw).expect("read");
        parse_http_response(&raw).expect("response")
    }

    fn parse_http_response(raw: &str) -> Result<(i64, String), String> {
        let (head, body) = raw.split_once("\r\n\r\n").ok_or("no body")?;
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse().ok())
            .ok_or("no status")?;
        Ok((status, body.to_string()))
    }

    /// A run whose grant names the `mcp` namespace, so a server bound to an
    /// ephemeral port is authorized whatever authority it receives.
    fn start() -> i64 {
        let spec = AgentSpec::parse(
            r#"{"goal":"mcp-server","model":"mock/echo","endpoint":"mock:","allow":["mcp"]}"#,
        )
        .expect("valid spec");
        run::alloc_run(spec, crate::journal::new_run_id(), None).expect("alloc")
    }

    fn with_registry<T>(work: impl FnOnce() -> T) -> T {
        let _guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _registry = tools::REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        spectra_runtime::ffi::clear_host_functions();
        spectra_runtime::register();
        crate::register();
        clear_http_transport();
        tools::clear();
        let result = work();
        clear_http_transport();
        tools::clear();
        result
    }

    #[test]
    fn tools_list_serves_the_derived_descriptors() {
        with_registry(|| {
            assert!(tools::register(
                "double".to_string(),
                doubling_tool as *const () as usize as i64,
                "Doubles an integer".to_string(),
                r#"{"type":"object","properties":{"n":{"type":"integer"}}}"#.to_string(),
                r#"["spectra.std.fs.fs_write"]"#,
            ).expect("register"));
            let serving = start();
            let request = wire::request(1, "tools/list", json!({}));
            let response: Value =
                serde_json::from_str(&handle(serving, &request).expect("served")).expect("json");
            let tools = response["result"]["tools"].as_array().expect("tools");
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0]["name"], "double");
            assert_eq!(tools[0]["description"], "Doubles an integer");
            assert_eq!(tools[0]["inputSchema"]["properties"]["n"]["type"], "integer");
            assert_eq!(
                tools[0]["annotations"]["destructiveHint"], true,
                "the compiler-derived effect is a catalog sink"
            );
            assert_eq!(tools[0]["annotations"]["readOnlyHint"], false);

            // A notification has no response.
            assert_eq!(
                handle(serving, &wire::notification("notifications/initialized")).expect("served"),
                ""
            );
            run::take_run(serving).expect("end");
        });
    }

    #[test]
    fn a_third_party_client_calls_a_registered_tool_over_real_http() {
        with_registry(|| {
            assert!(tools::register(
                "double".to_string(),
                doubling_tool as *const () as usize as i64,
                "Doubles an integer".to_string(),
                r#"{"type":"object"}"#.to_string(),
                "[]",
            ).expect("register"));
            let serving = start();
            let authority = serve(serving, "127.0.0.1:0").expect("served");
            assert!(authority.starts_with("127.0.0.1:"), "{authority}");

            let (status, body) = third_party_post(
                &authority,
                &wire::request(7, "tools/call", json!({"name": "double", "arguments": {"n": 21}})),
            );
            assert_eq!(status, 200);
            let response: Value = serde_json::from_str(&body).expect("json");
            assert_eq!(response["id"], 7);
            assert_eq!(response["result"]["isError"], false);
            assert_eq!(response["result"]["content"][0]["text"], "42");

            // An unknown tool is a tool-level failure, not a protocol failure.
            let (status, body) = third_party_post(
                &authority,
                &wire::request(8, "tools/call", json!({"name": "missing", "arguments": {}})),
            );
            assert_eq!(status, 200);
            let response: Value = serde_json::from_str(&body).expect("json");
            assert_eq!(response["result"]["isError"], true);
            assert!(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("missing"),
                "{response}"
            );

            // The governed dispatch charged the serving run for the one call
            // that reached a tool.
            let report = run::take_run(serving).expect("end");
            assert_eq!(report.tool_calls, 1);

            // Once the run ends, its surface is gone: the listener answers 410
            // and stops serving.
            let (status, body) = third_party_post(
                &authority,
                &wire::request(9, "tools/call", json!({"name": "double", "arguments": {"n": 1}})),
            );
            assert_eq!(status, 410, "{body}");
        });
    }

    #[test]
    fn the_client_round_trip_uses_the_same_governed_dispatch() {
        with_registry(|| {
            assert!(tools::register(
                "double".to_string(),
                doubling_tool as *const () as usize as i64,
                "Doubles an integer".to_string(),
                r#"{"type":"object"}"#.to_string(),
                "[]",
            ).expect("register"));
            let serving = start();
            let authority = serve(serving, "127.0.0.1:0").expect("served");
            clear_http_transport();
            assert!(set_http_transport(TcpTransport));

            let url = format!("http://{authority}/mcp");
            let document =
                crate::mcp::client::connect(serving, &url).expect("discovered over HTTP");
            let discovered: Value = serde_json::from_str(&document).expect("json");
            let names: Vec<&str> = discovered["tools"]
                .as_array()
                .expect("tools")
                .iter()
                .filter_map(|tool| tool["remoteName"].as_str())
                .collect();
            assert_eq!(
                names,
                ["double"],
                "the client sees the server's compiled tool exactly once: {document}"
            );
            let remote_name = discovered["tools"][0]["name"].as_str().expect("name");

            let result = crate::act::tool_call(serving, remote_name, r#"{"n":4}"#)
                .expect("the remote tool executed over HTTP");
            assert_eq!(result, "8");
            let state = run::take_run(serving).expect("end");
            assert_eq!(state.tool_calls, 2, "the local and the remote call are charged");
        });
    }

    #[test]
    fn a_run_level_refusal_is_a_jsonrpc_error() {
        with_registry(|| {
            assert!(tools::register(
                "double".to_string(),
                doubling_tool as *const () as usize as i64,
                "Doubles an integer".to_string(),
                r#"{"type":"object"}"#.to_string(),
                r#"["spectra.std.fs.fs_write"]"#,
            ).expect("register"));
            // The run grants nothing, so the tool's derived effect is outside it.
            let spec = AgentSpec::parse(
                r#"{"goal":"ungranted","model":"mock/echo","endpoint":"mock:","allow":[]}"#,
            )
            .expect("valid spec");
            let serving = run::alloc_run(spec, crate::journal::new_run_id(), None).expect("alloc");
            let request = wire::request(1, "tools/call", json!({"name": "double", "arguments": {"n": 1}}));
            let response: Value =
                serde_json::from_str(&handle(serving, &request).expect("served")).expect("json");
            assert_eq!(response["error"]["code"], RUN_REFUSED);
            assert!(
                response["error"]["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("does not authorize"),
                "{response}"
            );
            run::take_run(serving).expect("end");
        });
    }

    /// A peer that connects and then says nothing has to end its own
    /// connection, not the listener.
    ///
    /// The listener allows five seconds for a request (the read timeout in
    /// `net`), so this test deliberately waits past it: before the fix, the
    /// timed-out connection tripped the accept loop's error arm, the listener
    /// thread returned, the port was released and the endpoint was dead for the
    /// rest of the process lifetime — while the run still advertised the
    /// authority as served. The connecting peer is a port scanner or a stalled
    /// client, so the failure was reachable without a governing run doing
    /// anything wrong.
    #[test]
    fn a_stalled_connection_does_not_take_the_listener_down() {
        with_registry(|| {
            let serving = start();
            let authority = serve(serving, "127.0.0.1:0").expect("served");

            let stalled = TcpStream::connect(&authority).expect("connect");
            std::thread::sleep(std::time::Duration::from_millis(5_400));
            drop(stalled);

            // The same endpoint still answers. The connect itself fails when
            // the accept loop died, so this is the whole assertion.
            let (status, body) =
                third_party_post(&authority, &wire::request(5, "tools/list", json!({})));
            assert_eq!(status, 200, "{body}");
            run::take_run(serving).expect("end");
        });
    }

    #[test]
    fn malformed_requests_are_typed_failures_or_protocol_errors() {
        with_registry(|| {
            let serving = start();
            let error = handle(serving, "not json").expect_err("typed failure");
            assert_eq!(error.kind(), "mcp_error");
            let response = dispatch(serving, &json!({"jsonrpc": "2.0", "id": 3, "method": "nope"}));
            assert_eq!(response["error"]["code"], METHOD_NOT_FOUND);
            let response =
                dispatch(serving, &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call"}));
            assert_eq!(response["error"]["code"], INVALID_PARAMS);
            run::take_run(serving).expect("end");
        });
    }
}
