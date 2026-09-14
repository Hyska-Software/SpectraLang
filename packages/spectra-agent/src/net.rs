//! Minimal HTTP/1.1 listener shared by the in-crate protocol adapters
//! (R-3218 MCP, R-3219 A2A).
//!
//! This crate cannot depend on `spectra-api`, so an adapter that must be
//! reachable by a third-party client carries a listener of its own. The
//! listener is deliberately small — one request per connection, no TLS, no
//! keep-alive, no authentication, no back-pressure — because the transport
//! belongs to the embedding host: an application that needs those properties
//! serves the same per-request handler from its own stack (`std.api.server`),
//! and fronts the in-crate listener otherwise.
//!
//! What this module owns is framing only: it parses the request, hands the
//! method, the path and the body to the adapter's handler, and writes the
//! handler's response. It makes no protocol decision, so a second protocol
//! adapter reuses it instead of duplicating a listener.
//!
//! Two behaviors are load-bearing and shared by every caller:
//!
//! * the request is fully read **before** anything is decided, so a peer always
//!   receives a response instead of observing a reset connection (Windows
//!   resets a socket that is closed with unread bytes);
//! * a handler warning `stop` ends the loop after its response is written,
//!   which is how a listener whose serving run has ended goes away;
//! * a connection that fails mid-read (a stalled peer hitting the read
//!   timeout, a reset, a half-open socket) ends **that connection**, never the
//!   loop, so one bad peer cannot silently take the endpoint down while the
//!   process still advertises its authority.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

/// Upper bound on one request body. A peer cannot make an adapter allocate
/// without limit.
pub(crate) const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Upper bound on the request header block.
pub(crate) const MAX_HEADER_BYTES: usize = 64 * 1024;

/// One parsed request: the method, the path and the body.
pub(crate) struct Request {
    pub method: String,
    pub path: String,
    pub body: String,
}

/// One response with a single body.
pub(crate) struct Response {
    pub status: u16,
    pub reason: &'static str,
    pub content_type: &'static str,
    pub body: String,
    /// Set when the listener must stop after writing this response.
    pub stop: bool,
}

impl Response {
    /// A `200 OK` JSON document.
    pub(crate) fn json(body: String) -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type: "application/json",
            body,
            stop: false,
        }
    }

    /// A body-less-protocol response (a plain-text reason, or an error page).
    pub(crate) fn text(status: u16, reason: &'static str, body: impl Into<String>) -> Self {
        Self {
            status,
            reason,
            content_type: "text/plain",
            body: body.into(),
            stop: false,
        }
    }

    /// Marks this response as the listener's last.
    pub(crate) fn stop(mut self) -> Self {
        self.stop = true;
        self
    }
}

/// One adapter's request handler.
pub(crate) type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync>;

/// Binds `bind` and returns the listener with its authority (`host:port`).
///
/// Split from [`spawn`] so a caller that must know its own address before it
/// can answer — the A2A card names the endpoint URL — can build the handler
/// after binding.
pub(crate) fn bind(bind: &str) -> Result<(TcpListener, String), String> {
    let listener =
        TcpListener::bind(bind).map_err(|error| format!("could not bind '{bind}': {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("could not read the listener address: {error}"))?;
    Ok((listener, address.to_string()))
}

/// Answers requests on a dedicated thread named `<thread>-<authority>`.
pub(crate) fn spawn(listener: TcpListener, thread: &str, handler: Handler) -> Result<(), String> {
    let authority = listener
        .local_addr()
        .map(|address| address.to_string())
        .unwrap_or_default();
    std::thread::Builder::new()
        .name(format!("{thread}-{authority}"))
        .spawn(move || accept_loop(listener, handler))
        .map_err(|error| format!("could not start the listener thread: {error}"))?;
    Ok(())
}

/// [`bind`] + [`spawn`] for a caller that does not need its own authority.
pub(crate) fn serve(address: &str, thread: &str, handler: Handler) -> Result<String, String> {
    let (listener, authority) = bind(address)?;
    spawn(listener, thread, handler)?;
    Ok(authority)
}

/// Serves connections until a handler says `stop` or the process exits.
fn accept_loop(listener: TcpListener, handler: Handler) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

        // The request is read before anything is decided, so the peer always
        // receives a response instead of seeing a reset connection.
        //
        // A connection that failed mid-read (the read timeout above, a reset,
        // a half-open socket) is that connection's problem, never the
        // listener's: one stalled or abortive peer must not take the endpoint
        // down for everyone else. The listener stops for exactly two reasons —
        // a handler that warns `stop` (the 410 a run-scoped adapter answers
        // once its run has ended) or the process exiting — which is the
        // lifecycle the adapters document.
        let (response, stop) = match read_request(&mut stream) {
            Ok(Some(request)) => {
                let response = handler(&request);
                (render(&response), response.stop)
            }
            Ok(None) => (
                render(&Response::text(400, "Bad Request", "malformed HTTP request")),
                false,
            ),
            Err(_) => (
                render(&Response::text(
                    408,
                    "Request Timeout",
                    "the connection did not deliver a complete request",
                )),
                false,
            ),
        };
        // A peer that vanished while the response was written is gone; the
        // next connection is unaffected.
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        if stop {
            break;
        }
    }
}

/// Reads one request: the request line, the headers and the body.
fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(position) = header_terminator(&buffer) {
            break position;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return Ok(None);
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let path = parts.next().unwrap_or("").to_string();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    if content_length > MAX_BODY_BYTES {
        return Ok(None);
    }

    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    if body.len() < content_length {
        return Ok(None);
    }
    body.truncate(content_length);
    Ok(Some(Request {
        method,
        path,
        body: String::from_utf8_lossy(&body).to_string(),
    }))
}

/// Offset of the `\r\n\r\n` that ends the header block.
fn header_terminator(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// One HTTP/1.1 response with a single body.
fn render(response: &Response) -> String {
    format!(
        "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\n\
         connection: close\r\n\r\n{}",
        response.status,
        response.reason,
        response.content_type,
        response.body.len(),
        response.body
    )
}
