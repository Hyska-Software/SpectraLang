//! MCP wire helpers shared by the client and the server (R-3218).
//!
//! The adapter speaks JSON-RPC 2.0 over the streamable-HTTP transport, the
//! only transport the language supports (`docs/agent-platform.md` records why
//! stdio is refused). This module owns the framing, the endpoint identity and
//! the per-server capability derivation.
//!
//! Nothing here interprets a peer's payload: a method result is returned as a
//! `serde_json::Value` (data). The client tags every remote description and
//! schema in the run's taint ledger before it hands anything back to Spectra
//! code, so no text a peer controls is ever executed, evaluated or matched
//! against a control-flow decision.

use serde_json::{json, Value};

use crate::error::AgentError;

/// Protocol version this adapter speaks. The version is also sent on every
/// request after `initialize` (the `mcp-protocol-version` header the 2025-06-18
/// revision defines).
pub(crate) const PROTOCOL_VERSION: &str = "2025-06-18";

/// An MCP endpoint and the capability derived from its identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    /// The URL every request is posted to, exactly as the caller wrote it.
    pub(crate) url: String,
    /// `host[:port]`, lowercased: the server identity, taken from the URL
    /// authority and never from a server-supplied field (a peer must not be
    /// able to name its own capability).
    pub(crate) authority: String,
    /// The per-server grant derived from the identity: `mcp.<authority>`.
    /// The tool facade uses the `.`-namespace grant form the capability seam
    /// already matches, so `allow: ["mcp"]` grants every server and
    /// `allow: ["mcp.api.example.com"]` grants exactly that host.
    pub(crate) capability: String,
}

impl Endpoint {
    /// Parses an endpoint URL and derives its capability.
    pub(crate) fn parse(url: &str) -> Result<Self, AgentError> {
        let trimmed = url.trim();
        let (scheme, rest) = trimmed.split_once("://").ok_or_else(|| {
            AgentError::Mcp(format!(
                "MCP endpoint '{url}' must be an absolute URL of the form \
                 http://host[:port]/path"
            ))
        })?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(AgentError::Mcp(format!(
                "MCP endpoint '{url}' uses scheme '{scheme}'; only http and https are \
                 supported (stdio requires subprocess support the language does not have)"
            )));
        }
        let authority = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if authority.is_empty()
            || authority.contains('@')
            || authority
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(AgentError::Mcp(format!(
                "MCP endpoint '{url}' has no usable host authority; write \
                 http://host[:port]/path"
            )));
        }
        Ok(Self {
            capability: format!("mcp.{authority}"),
            authority,
            url: trimmed.to_string(),
        })
    }

    /// Namespace prefix of the tools this server contributes.
    fn namespace(&self) -> String {
        let mut rendered = String::from("mcp__");
        for character in self.authority.chars() {
            if character.is_ascii_alphanumeric() {
                rendered.push(character);
            } else {
                rendered.push('_');
            }
        }
        rendered.push_str("__");
        rendered
    }

    /// The process-local dispatch name of one of this server's tools.
    ///
    /// The authority is sanitized so the derived name is addressable from a
    /// model (and from `tool_call`); the remote name is kept verbatim, because
    /// two distinct remote names must never collide in this registry, and MCP
    /// already requires a server to name its tools uniquely.
    pub(crate) fn tool_name(&self, remote_name: &str) -> String {
        format!("{}{remote_name}", self.namespace())
    }
}

/// One JSON-RPC request document.
pub(crate) fn request(id: i64, method: &str, params: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

/// One JSON-RPC notification document (no id, no response).
pub(crate) fn notification(method: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "method": method,
    })
    .to_string()
}

/// A JSON-RPC success response.
pub(crate) fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// A JSON-RPC error response.
pub(crate) fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    })
}

/// Parses a response body into JSON, accepting both response modes the
/// streamable-HTTP transport defines: a single JSON document, or a
/// `text/event-stream` body whose `data:` lines carry the documents.
///
/// An empty body (a notification's `202 Accepted`) is `Value::Null`.
pub(crate) fn parse_body(body: &str) -> Result<Value, AgentError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return serde_json::from_str(trimmed)
            .map_err(|error| AgentError::Mcp(format!("peer sent malformed JSON: {error}")));
    }
    // Server-sent events: the last `data:` payload is the response.
    let mut payload = None;
    for line in trimmed.lines() {
        let line = line.trim_end();
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if !data.is_empty() {
                payload = Some(data.to_string());
            }
        }
    }
    let Some(payload) = payload else {
        return Err(AgentError::Mcp(
            "peer sent neither a JSON document nor an SSE data line".to_string(),
        ));
    };
    serde_json::from_str(&payload)
        .map_err(|error| AgentError::Mcp(format!("peer sent a malformed SSE payload: {error}")))
}

/// Extracts a JSON-RPC result, mapping a peer-reported error to a typed
/// failure that carries the peer's own text as data.
pub(crate) fn result_of(value: Value, method: &str) -> Result<Value, AgentError> {
    if let Some(error) = value.get("error") {
        let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("no message");
        return Err(AgentError::Mcp(format!(
            "the MCP peer refused '{method}' with JSON-RPC error {code}: {message}"
        )));
    }
    value.get("result").cloned().ok_or_else(|| {
        AgentError::Mcp(format!("the MCP peer answered '{method}' without a result"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capability_is_derived_from_the_url_authority() {
        let endpoint = Endpoint::parse("HTTPS://API.Example.COM:8443/mcp?x=1").expect("endpoint");
        assert_eq!(endpoint.authority, "api.example.com:8443");
        assert_eq!(endpoint.capability, "mcp.api.example.com:8443");
        assert_eq!(endpoint.tool_name("search"), "mcp__api_example_com_8443__search");
        // A peer-supplied identity is impossible: there is no other input.
        let local = Endpoint::parse("http://127.0.0.1:9000").expect("endpoint");
        assert_eq!(local.capability, "mcp.127.0.0.1:9000");
        assert_eq!(local.tool_name("a.b"), "mcp__127_0_0_1_9000__a.b");
    }

    #[test]
    fn only_absolute_http_urls_are_accepted() {
        for rejected in ["", "api.example.com/mcp", "stdio:server", "file:///tmp/x"] {
            let error = Endpoint::parse(rejected).expect_err("rejected");
            assert_eq!(error.kind(), "mcp_error", "{error}");
        }
    }

    #[test]
    fn sse_and_plain_bodies_both_parse() {
        assert_eq!(
            parse_body(r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#)
                .expect("json")["id"],
            1
        );
        let sse = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n\n";
        assert_eq!(parse_body(sse).expect("sse")["result"]["tools"], json!([]));
        assert_eq!(parse_body("").expect("empty"), Value::Null);
        assert!(parse_body("event: ping\n").is_err());
    }

    #[test]
    fn a_peer_error_is_carried_as_a_typed_failure() {
        let error = result_of(
            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}}),
            "tools/list",
        )
        .expect_err("peer error");
        assert_eq!(error.kind(), "mcp_error");
        assert!(error.detail().contains("nope"), "{error}");
    }
}
