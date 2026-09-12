//! Host adapters for the two `std.agent` injection seams (plan adaptation 12).
//!
//! `spectra-agent` owns the seams — the providers' [`HttpTransport`] and the
//! OpenTelemetry GenAI [`TraceSink`] — but it cannot depend on this crate,
//! which aggregates it. The real implementations therefore live here, over the
//! client owned by `spectra.api.client` and the tracer owned by
//! `spectra.api.trace`:
//!
//! * [`ApiHttpTransport`] sends every provider and MCP JSON POST through this
//!   crate's [`HttpClient`]. The client owns the TLS trust anchors, the
//!   connection pool, the redirect policy and the SSRF policy, so an agent
//!   request cannot bypass any of them. The installed default permits private
//!   networks (see [`install_agent_host_adapters`]).
//! * [`ApiTraceSink`] forwards each `std.agent` GenAI span into
//!   [`spectra_runtime::tracing`], so the pinned conventions version and the
//!   span attributes reach the same exporter the `std.api.trace` hosts drive.
//!
//! [`install_agent_host_adapters`] is called from [`crate::register`], so a
//! process that registers the host calls also wires both adapters; tests
//! install fakes through the same public seams.

use crate::client::{ClientConfig, ClientRequest, HttpClient};
use spectra_agent::{HttpTransport, Span, TraceSink, TransportResponse};
use spectra_runtime::tracing::{self, SpanKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The providers' HTTP transport over this crate's client.
///
/// One client per adapter: TLS, pooling, redirects and SSRF are the client's
/// exactly as `spectra.api.client` applies them, and the adapter adds nothing
/// to the wire beyond the caller's own headers.
pub struct ApiHttpTransport {
    client: HttpClient,
}

impl ApiHttpTransport {
    /// Builds the adapter over a fresh client with `config`.
    pub fn new(config: ClientConfig) -> Self {
        Self {
            client: HttpClient::new(config),
        }
    }

    /// The underlying client (statistics, TLS or SSRF policy changes).
    pub fn client(&self) -> &HttpClient {
        &self.client
    }
}

impl HttpTransport for ApiHttpTransport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<TransportResponse, String> {
        let mut request = ClientRequest::new("POST", url).with_body(body.as_bytes().to_vec());
        let declared_content_type = headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"));
        for (name, value) in headers {
            request = request.with_header(name.clone(), value.clone());
        }
        // The transport is JSON-only by contract; a caller that did not declare
        // the media type still sends a well-formed request.
        if !declared_content_type {
            request = request.with_header("content-type", "application/json");
        }
        let response = self
            .client
            .request(request)
            .map_err(|error| error.to_string())?;
        Ok(TransportResponse {
            status: i64::from(response.status_code),
            body: String::from_utf8_lossy(&response.body.bytes()).into_owned(),
        })
    }
}

/// Forwards `std.agent` GenAI spans into the runtime's tracer.
///
/// Content capture is off: [`TraceSink::captures_content`] keeps the trait
/// default (`false`), so the agent layer never attaches prompts, completions
/// or tool payloads to a span, and this sink never reads `Span::content` or
/// `Span::result` even if a future caller set them.
///
/// Mapping, per recorded span:
///
/// * the span name and kind — `chat {model}` is a client call, the other
///   operations are in-process work;
/// * every `gen_ai.*` attribute the agent layer attached
///   (`gen_ai.operation.name`, `gen_ai.agent.name`, `gen_ai.conversation.id`,
///   `gen_ai.request.model`, `gen_ai.tool.name`) verbatim;
/// * `gen_ai.conventions.version` and `gen_ai.conventions.schema_url` — the
///   pinned `1.34.0` identity, which the runtime tracer has no dedicated field
///   for;
/// * `spectra.agent.step`, the run's effect step, when the span belongs to one
///   (no semantic convention spells it).
///
/// No status is invented: the agent layer records a span for an activity it
/// has already completed but carries no outcome for the whole run, so the
/// span keeps the runtime's `Unset` status.
pub struct ApiTraceSink;

impl TraceSink for ApiTraceSink {
    fn record(&self, span: &Span) {
        // No active tracing configuration means the runtime has nowhere to send
        // the span; it is dropped exactly like a `std.api.trace` host span.
        let Ok(id) = tracing::span_start(&span.name, span_kind(span.operation)) else {
            return;
        };
        for (key, value) in &span.attributes {
            let _ = tracing::span_set_attribute(id, key, value);
        }
        if let Some(step) = span.step {
            let _ = tracing::span_set_attribute_int(id, "spectra.agent.step", step as i64);
        }
        let _ = tracing::span_set_attribute(
            id,
            "gen_ai.conventions.version",
            span.conventions_version,
        );
        let _ = tracing::span_set_attribute(
            id,
            "gen_ai.conventions.schema_url",
            span.schema_url,
        );
        let _ = tracing::span_end(id);
    }
}

/// The span kind an operation maps to.
fn span_kind(operation: &str) -> SpanKind {
    match operation {
        // The model call is the one operation that leaves the process.
        "chat" => SpanKind::Client,
        _ => SpanKind::Internal,
    }
}

/// The adapters were installed once; the sink seam replaces on every set, so a
/// later `register()` must not displace a sink an embedder attached.
static ADAPTERS_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Installs both adapters with the agent platform's client policy.
///
/// This is what [`crate::register`] calls. The policy is the client's, with
/// private networks permitted: every agent endpoint is authored program data
/// (the spec's `endpoint`, the `mcp_connect` URL), and the language's MCP
/// design serves and consumes an in-process loopback (R-3218), so a local
/// model server has to be reachable. An embedder that wants the client's
/// strict SSRF default, or any other client policy, installs its own adapter
/// with [`install_agent_host_adapters_with`] before registering: the transport
/// seam is first-install-wins, so the earlier installation is the one in use.
/// Returns `true` when this call installed the transport.
pub fn install_agent_host_adapters() -> bool {
    install_agent_host_adapters_with(ClientConfig::default().allow_private_networks(true))
}

/// Installs both adapters with an explicit client configuration.
///
/// The transport seam is first-install-wins, so `true` means this call
/// installed it and a later `register()` — or a test's fake — never displaces
/// a transport already in use. The sink seam replaces instead, so the sink is
/// installed once per process.
pub fn install_agent_host_adapters_with(config: ClientConfig) -> bool {
    let transport_installed = spectra_agent::set_http_transport(ApiHttpTransport::new(config));
    if !ADAPTERS_INSTALLED.swap(true, Ordering::AcqRel) {
        spectra_agent::set_trace_sink(Some(Arc::new(ApiTraceSink)));
    }
    transport_installed
}
