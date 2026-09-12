//! `std.agent.mcp`: consume and expose Model Context Protocol tools over HTTP
//! (R-3218).
//!
//! Two directions, one governed path:
//!
//! * [`client`] discovers a remote server's tools (`tools/list`) and registers
//!   each one as a first-class entry of this process's tool registry, so
//!   `tool_call` — and therefore `act` — can invoke a remote tool exactly like
//!   a compiled `#[agent_tool]`: same budget charge, same journal step, same
//!   taint provenance, same grant check.
//! * [`server`] answers `tools/list` and `tools/call` from that registry, so a
//!   Spectra project's compiled tools are callable by any MCP client. A
//!   `tools/call` executes inside the run with the run's capability set and
//!   through the governed dispatch.
//!
//! # Untrusted by construction
//!
//! Everything a remote server says — tool names, descriptions and JSON
//! schemas — is data. The client never interprets it: descriptions and schemas
//! are recorded in the run's taint ledger under the server's capability as
//! origin (R-3223) before anything is returned to Spectra code, and a tool
//! result is untrusted through `observe_transcript` on its way to the model.
//! A description that contains instructions therefore cannot influence control
//! flow: the only decisions the adapter makes are protocol ones (which method,
//! which registered name), and a remote tool's derived effect is the
//! per-server capability `mcp.<authority>`, which the run must already grant.
//!
//! # Transport
//!
//! HTTP is the transport, and it is the host's: requests go through the
//! injected [`HttpTransport`](crate::provider::transport::HttpTransport) so
//! the embedding application keeps ownership of TLS, pooling and SSRF policy.
//!
//! One exception is deliberate and cannot leave the process: when no host
//! transport is installed, a URL whose authority was bound by
//! [`server::serve`] in this same process is served over an in-process
//! loopback, because that request never reaches the network. A URL that is not
//! a live local listener still fails closed with "no HTTP transport
//! installed"; the agent layer never opens a socket the host did not provide.

pub(crate) mod client;
pub(crate) mod server;
pub(crate) mod wire;

use crate::error::AgentError;
use crate::provider::transport::{http_transport, TransportResponse};

use wire::PROTOCOL_VERSION;

/// Posts one JSON-RPC document to `url` and returns the response body.
///
/// The injected transport is consulted first and always wins, so a host that
/// installed one owns every byte on the wire. Only when there is none does the
/// in-process loopback answer, and only for a server this process is serving.
pub(crate) fn post(url: &str, body: &str) -> Result<String, AgentError> {
    let headers = vec![
        ("content-type".to_string(), "application/json".to_string()),
        (
            "accept".to_string(),
            "application/json, text/event-stream".to_string(),
        ),
        (
            "mcp-protocol-version".to_string(),
            PROTOCOL_VERSION.to_string(),
        ),
    ];
    let response: TransportResponse = match http_transport() {
        Some(transport) => transport.post_json(url, &headers, body).map_err(|error| {
            AgentError::Mcp(format!("MCP request to '{url}' failed: {error}"))
        })?,
        None => server::loopback_post(url, body).ok_or_else(|| {
            AgentError::Mcp(format!(
                "no HTTP transport is installed, so the MCP request to '{url}' cannot be sent; \
                 the embedding application installs one (spectra-api does not yet), or serve \
                 the endpoint from this process with mcp_serve"
            ))
        })?,
    };
    if response.status == 202 {
        // A notification is accepted without a body.
        return Ok(String::new());
    }
    if !(200..300).contains(&response.status) {
        return Err(AgentError::Mcp(format!(
            "MCP endpoint '{url}' answered HTTP {}",
            response.status
        )));
    }
    Ok(response.body)
}
