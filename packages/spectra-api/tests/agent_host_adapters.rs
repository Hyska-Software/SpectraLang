//! End-to-end proof for the host-installed `std.agent` adapters (R-3221 T4).
//!
//! Two seams are installed by `spectra_api::register()`:
//!
//! * the real `HttpTransport`, which these tests exercise by running an agent
//!   whose provider endpoint is a local `TcpListener` speaking the
//!   OpenAI-compatible chat-completions format; and
//! * the GenAI `TraceSink`, which these tests exercise by running an agent
//!   while the runtime's OTLP exporter points at a second local listener, then
//!   asserting the exported span names/attributes and the absence of content.
//!
//! A separate integration-test binary keeps the process-global seams, host
//! registry and trace configuration free of the crate's own unit tests.

use spectra_api::agent_transport::{
    install_agent_host_adapters, install_agent_host_adapters_with, ApiTraceSink,
};
use spectra_api::client::ClientConfig;
use spectra_agent::TraceSink;
use spectra_runtime::ffi::{lookup_host_function, SpectraHostCallContext, HOST_STATUS_SUCCESS};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{mpsc, LazyLock, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// The seams are process-global, so the tests here never run concurrently.
fn test_guard() -> MutexGuard<'static, ()> {
    static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
    LOCK.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ── host-call harness ────────────────────────────────────────────────────

fn call(name: &str, args: &[i64]) -> (i32, i64) {
    let function = lookup_host_function(name).expect("host registered");
    let mut results = [0_i64];
    let mut ctx = SpectraHostCallContext {
        args: args.as_ptr(),
        arg_len: args.len(),
        results: results.as_mut_ptr(),
        result_len: results.len(),
        invoke_fn: None,
    };
    (function(&mut ctx), results[0])
}

fn spectra_string(value: &str) -> i64 {
    unsafe { spectra_runtime::tracing::alloc_string(value) }
}

/// The tagged `Result` pointer: word 0 is the tag, word 1 the payload.
fn tag(value: i64) -> i64 {
    unsafe { *(value as *const i64) }
}

fn payload(value: i64) -> i64 {
    unsafe { *(value as *const i64).add(1) }
}

fn spectra_text(value: i64) -> String {
    assert_ne!(value, 0, "expected a Spectra string pointer");
    let mut bytes = Vec::new();
    let mut offset = 0usize;
    loop {
        let byte = unsafe { *(value as *const u8).add(offset) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
        offset += 1;
    }
    String::from_utf8(bytes).expect("host strings are UTF-8")
}

fn start_agent(spec: &str) -> i64 {
    let (status, result) = call("spectra.std.agent.agent_start", &[spectra_string(spec)]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert_eq!(tag(result), 0, "agent_start refused the spec");
    payload(result)
}

/// The message of a tagged `Err` result, whose payload is a structured
/// `std.error` value rather than a string.
fn error_message(value: i64) -> String {
    assert_eq!(tag(value), 1, "expected an Err result");
    let (status, message) = call("spectra.std.error.message", &[payload(value)]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    spectra_text(message)
}

fn ask(run: i64, prompt: &str) -> String {
    let (status, task) = call("spectra.std.agent.ask", &[run, spectra_string(prompt)]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    let settled = spectra_runtime::stdlib::block_on_task_value(task).expect("ask resolves");
    assert_eq!(tag(settled), 0, "ask failed");
    spectra_text(payload(settled))
}

/// A journal-free spec: these tests exercise the adapters, not durability, and
/// must not write into the process cwd.
fn spec(goal: &str, model: &str, endpoint: &str) -> String {
    format!(r#"{{"goal":"{goal}","model":"{model}","endpoint":"{endpoint}","journal":""}}"#)
}

// ── local HTTP recorder ──────────────────────────────────────────────────

/// A loopback HTTP/1.1 server that answers every request with the same raw
/// response and records the request bytes it received.
struct Recorder {
    address: SocketAddr,
    stop: mpsc::Sender<()>,
    join: JoinHandle<Vec<u8>>,
}

impl Recorder {
    fn finish(self) -> Vec<u8> {
        let _ = self.stop.send(());
        self.join.join().expect("recorder thread")
    }
}

/// Byte-substring search: the OTLP payload is protobuf, not text.
fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let body_start = loop {
        let read = stream.read(&mut buffer).expect("recorder read");
        assert!(read > 0, "recorder request truncated");
        request.extend_from_slice(&buffer[..read]);
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let head = String::from_utf8_lossy(&request[..body_start]).to_ascii_lowercase();
    let content_length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while request.len() < body_start + content_length {
        let read = stream.read(&mut buffer).expect("recorder body read");
        assert!(read > 0, "recorder body truncated");
        request.extend_from_slice(&buffer[..read]);
    }
    request.truncate(body_start + content_length);
    request
}

fn spawn_recorder(response: Vec<u8>) -> Recorder {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind recorder");
    let address = listener.local_addr().expect("recorder address");
    listener
        .set_nonblocking(true)
        .expect("recorder nonblocking mode");
    let (stop, stopped) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut requests = Vec::new();
        loop {
            match stopped.try_recv() {
                Ok(()) | Err(mpsc::TryRecvError::Disconnected) => return requests,
                Err(mpsc::TryRecvError::Empty) => {}
            }
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("recorder accept: {error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("recorder read timeout");
            requests.extend_from_slice(&read_request(&mut stream));
            stream
                .write_all(&response)
                .expect("recorder response write");
        }
    });
    Recorder { address, stop, join }
}

fn json_response(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[test]
fn installed_transport_round_trips_the_openai_compatible_format_against_a_local_listener() {
    let _guard = test_guard();
    spectra_runtime::ffi::clear_host_functions();
    spectra_agent::clear_http_transport();
    // The exact adapter `register()` installs.
    assert!(
        install_agent_host_adapters(),
        "the hook must install the transport into an empty seam"
    );
    // Registration installs again, but the seam is first-install-wins, so the
    // transport above stays in place.
    spectra_api::register();

    let recorder = spawn_recorder(json_response(
        r#"{"choices":[{"message":{"role":"assistant","content":"local answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#,
    ));
    let run = start_agent(&spec(
        "provider round trip",
        "test/local",
        &format!("http://{}", recorder.address),
    ));
    let answer = ask(run, "hello local");
    assert_eq!(answer, "local answer");
    let (status, _) = call("spectra.std.agent.agent_end", &[run]);
    assert_eq!(status, HOST_STATUS_SUCCESS);

    let request = String::from_utf8(recorder.finish()).expect("HTTP request is UTF-8");
    assert!(
        request.starts_with("POST /v1/chat/completions HTTP/1.1"),
        "{request}"
    );
    assert!(request.contains(r#""model":"test/local""#), "{request}");
    assert!(request.contains(r#""content":"hello local""#), "{request}");
    assert!(
        request.to_ascii_lowercase().contains("content-type: application/json"),
        "{request}"
    );
}

#[test]
fn installed_trace_sink_exports_a_run_span_without_content_capture() {
    let _guard = test_guard();
    spectra_runtime::ffi::clear_host_functions();
    spectra_agent::clear_http_transport();
    // `register()` installs the sink adapter.
    spectra_api::register();
    assert!(
        !ApiTraceSink.captures_content(),
        "content capture must stay off by default"
    );

    let collector = spawn_recorder(
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    );
    let config = spectra_runtime::tracing::config_new(
        &format!("http://{}/v1/traces", collector.address),
        "spectra-api-agent-adapter-test",
    )
    .expect("trace configuration");
    spectra_runtime::tracing::config_start(config).expect("trace worker");

    const SECRET: &str = "PROMPT-CONTENT-MUST-NOT-BE-EXPORTED";
    let run = start_agent(&spec("trace goal", "mock/echo", "mock:"));
    let answer = ask(run, SECRET);
    assert!(answer.contains(SECRET), "the mock echoes the prompt");
    let (status, _) = call("spectra.std.agent.agent_end", &[run]);
    assert_eq!(status, HOST_STATUS_SUCCESS);

    spectra_runtime::tracing::flush().expect("trace flush");
    spectra_runtime::tracing::config_shutdown(config).expect("trace shutdown");
    let exported = collector.finish();

    assert!(contains(&exported, "invoke_agent"), "missing invoke_agent");
    assert!(contains(&exported, "chat mock/echo"), "missing chat span");
    assert!(
        contains(&exported, "gen_ai.operation.name"),
        "missing gen_ai.operation.name"
    );
    assert!(
        contains(&exported, "gen_ai.agent.name"),
        "missing gen_ai.agent.name"
    );
    assert!(
        contains(&exported, "gen_ai.conversation.id"),
        "missing gen_ai.conversation.id"
    );
    assert!(
        contains(&exported, "gen_ai.conventions.version"),
        "missing the pinned conventions version"
    );
    assert!(
        !contains(&exported, SECRET) && !contains(&exported, "mock echo:"),
        "content capture leaked prompt/response into the export"
    );
}

#[test]
fn a_strict_client_policy_is_reachable_and_enforced_by_the_transport() {
    let _guard = test_guard();
    spectra_runtime::ffi::clear_host_functions();
    spectra_agent::clear_http_transport();
    // The embedder hook wins the first-install-wins seam, so the installed
    // transport applies the client's strict SSRF default.
    assert!(install_agent_host_adapters_with(ClientConfig::default()));
    spectra_api::register();

    let recorder = spawn_recorder(json_response("{}"));
    let run = start_agent(&spec(
        "strict ssrf",
        "test/local",
        &format!("http://{}", recorder.address),
    ));
    let (status, task) = call("spectra.std.agent.ask", &[run, spectra_string("hello")]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    let settled = spectra_runtime::stdlib::block_on_task_value(task).expect("ask resolves");
    assert_eq!(
        tag(settled),
        1,
        "the strict policy must refuse a loopback endpoint"
    );
    let message = error_message(settled);
    assert!(message.contains("SSRF"), "{message}");
    let (status, _) = call("spectra.std.agent.agent_end", &[run]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert!(
        recorder.finish().is_empty(),
        "a policy-blocked request must never reach the listener"
    );
}
