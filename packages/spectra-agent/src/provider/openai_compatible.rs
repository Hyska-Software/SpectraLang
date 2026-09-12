//! OpenAI-compatible HTTP provider (R-3211 T2).
//!
//! The request goes through the injected [`HttpTransport`], so TLS, pooling
//! and SSRF policy remain owned by the transport implementation (adaptation 12).
//! Retries are bounded and only issued for requests that are idempotent by
//! construction: a deterministic run (`seed >= 0`) replays the identical
//! sampled request, and the retry count is recorded on the run.

use serde_json::{json, Value};

use super::{
    transport::{http_transport, TransportResponse},
    FinishReason, Provider, ProviderError, ProviderRequest, ProviderResponse, ToolCall, Usage,
};

/// Maximum additional attempts for an idempotent request.
const MAX_RETRIES: u64 = 2;

pub(crate) struct OpenAiCompatibleProvider {
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiCompatibleProvider {
    pub(crate) fn new(endpoint: String, model: String) -> Self {
        let api_key = std::env::var("SPECTRA_AGENT_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .ok()
            .filter(|key| !key.is_empty());
        Self {
            endpoint,
            model,
            api_key,
        }
    }

    fn chat_url(&self) -> Result<String, ProviderError> {
        if self.endpoint.is_empty() {
            return Err(ProviderError::NotConfigured(
                "no provider endpoint: set AgentSpec.endpoint, SPECTRA_AGENT_ENDPOINT or OPENAI_BASE_URL"
                    .to_string(),
            ));
        }
        Ok(format!(
            "{}/v1/chat/completions",
            self.endpoint.trim_end_matches('/')
        ))
    }

    fn request_body(&self, request: &ProviderRequest) -> Value {
        let messages = request
            .messages
            .iter()
            .map(|message| json!({ "role": message.role, "content": message.content }))
            .collect::<Vec<_>>();
        let model = if request.model.is_empty() {
            self.model.as_str()
        } else {
            request.model.as_str()
        };
        let mut body = json!({
            "model": model,
            "messages": messages,
        });
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(top_k) = request.top_k {
            // Not an OpenAI parameter, but OpenAI-compatible servers (vLLM,
            // llama.cpp) accept it; omitted when unset.
            body["top_k"] = json!(top_k);
        }
        if let Some(seed) = request.seed {
            body["seed"] = json!(seed);
        }
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if !request.tools.is_empty() {
            // OpenAI `tools` array. Each tool exposes one payload argument, so
            // the stored schema is the argument schema itself.
            let tools = request
                .tools
                .iter()
                .map(|tool| {
                    let parameters = serde_json::from_str::<Value>(&tool.input_schema)
                        .unwrap_or_else(|_| json!({ "type": "object" }));
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": parameters,
                        }
                    })
                })
                .collect::<Vec<_>>();
            body["tools"] = json!(tools);
        }
        // `top_k` has no OpenAI-compatible spelling; it is deliberately not
        // sent rather than mapped onto an unrelated parameter.
        if let Some(schema) = &request.json_schema {
            if let Ok(parsed) = serde_json::from_str::<Value>(schema) {
                body["response_format"] = json!({
                    "type": "json_schema",
                    "json_schema": { "name": "response", "schema": parsed, "strict": false }
                });
            }
        }
        body
    }

    fn post(&self, url: &str, body: &str) -> Result<TransportResponse, String> {
        let transport = http_transport().ok_or_else(|| {
            "no HTTP transport installed: spectra-api registers one at startup, or install a mock \
             transport in tests"
                .to_string()
        })?;
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        if let Some(key) = &self.api_key {
            headers.push(("authorization".to_string(), format!("Bearer {key}")));
        }
        transport.post_json(url, &headers, body)
    }
}

impl Provider for OpenAiCompatibleProvider {
    fn name(&self) -> &'static str {
        "openai_compatible"
    }

    fn is_configured(&self) -> bool {
        !self.endpoint.is_empty()
    }

    fn honors_seed(&self) -> bool {
        // OpenAI-compatible APIs accept `seed` on a best-effort basis and do
        // not confirm it was applied. Failing closed keeps the plan's
        // determinism promise: a run that requested a seed never observes a
        // silently non-deterministic result.
        false
    }

    fn complete(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let url = self.chat_url()?;
        let body = self.request_body(request).to_string();
        let idempotent = request.seed.is_some();
        let mut retries = 0u64;
        loop {
            match self.post(&url, &body) {
                Ok(response) if (200..300).contains(&response.status) => {
                    return parse_response(&response.body, retries);
                }
                Ok(response) => {
                    let retryable_status = response.status >= 500 || response.status == 429;
                    if idempotent && retryable_status && retries < MAX_RETRIES {
                        retries += 1;
                        continue;
                    }
                    return Err(ProviderError::Http {
                        status: response.status,
                        message: response.body,
                    });
                }
                Err(message) => {
                    if idempotent && retries < MAX_RETRIES {
                        retries += 1;
                        continue;
                    }
                    return Err(ProviderError::Transport(message));
                }
            }
        }
    }

    fn embed(&self, _text: &str) -> Result<Vec<f64>, ProviderError> {
        Err(ProviderError::NotConfigured(
            "the OpenAI-compatible provider has no embedding endpoint wired in R-3211; use the \
             mock provider for the deterministic embedding path"
                .to_string(),
        ))
    }
}

fn parse_response(body: &str, retries: u64) -> Result<ProviderResponse, ProviderError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| ProviderError::InvalidResponse(format!("response is not JSON: {error}")))?;
    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"));
    let tool_calls = message
        .and_then(|message| message.get("tool_calls"))
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| {
                    let function = call.get("function")?;
                    let name = function.get("name").and_then(Value::as_str)?;
                    let arguments = function
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    Some(ToolCall {
                        name: name.to_string(),
                        arguments: arguments.to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let text = match message
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
    {
        Some(content) => content.to_string(),
        // A tool-calling turn legitimately carries no string content.
        None if !tool_calls.is_empty() => String::new(),
        None => {
            return Err(ProviderError::InvalidResponse(
                "response has no choices[0].message.content string".to_string(),
            ));
        }
    };
    let usage = value.get("usage");
    let input_tokens = usage
        .and_then(|usage| usage.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|usage| usage.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let finish = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
        .map(|reason| match reason {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            other => FinishReason::Other(other.to_string()),
        })
        .unwrap_or(FinishReason::Stop);
    Ok(ProviderResponse {
        text,
        tool_calls,
        usage: Usage {
            input_tokens,
            output_tokens,
        },
        // Pricing for real models is R-3216's checked-in price table.
        cost_micros: 0,
        retries,
        finish,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::transport::{clear_http_transport, set_http_transport, HttpTransport};
    use crate::provider::Message;
    use std::sync::{Arc, Mutex};

    /// Serializes tests that mutate the process-global transport slot.
    static TRANSPORT_LOCK: Mutex<()> = Mutex::new(());

    struct ScriptedTransport {
        status: i64,
        body: String,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl ScriptedTransport {
        fn install(status: i64, body: &str) -> Arc<Mutex<Vec<String>>> {
            let calls = Arc::new(Mutex::new(Vec::new()));
            assert!(set_http_transport(Self {
                status,
                body: body.to_string(),
                calls: Arc::clone(&calls),
            }));
            calls
        }
    }

    impl HttpTransport for ScriptedTransport {
        fn post_json(
            &self,
            url: &str,
            _headers: &[(String, String)],
            body: &str,
        ) -> Result<TransportResponse, String> {
            self.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(format!("{url} {body}"));
            Ok(TransportResponse {
                status: self.status,
                body: self.body.clone(),
            })
        }
    }

    fn request(seed: Option<i64>) -> ProviderRequest {
        ProviderRequest {
            model: "gpt-test".to_string(),
            messages: vec![Message::user("hello")],
            temperature: seed.map(|_| 0.0),
            top_k: None,
            seed,
            json_schema: None,
            max_tokens: Some(16),
            tools: Vec::new(),
        }
    }

    fn provider() -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(
            "https://provider.invalid".to_string(),
            "gpt-test".to_string(),
        )
    }

    fn call_count(calls: &Arc<Mutex<Vec<String>>>) -> usize {
        calls.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    #[test]
    fn parses_a_well_formed_chat_completion() {
        let _guard = TRANSPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_http_transport();
        let calls = ScriptedTransport::install(
            200,
            r#"{"choices":[{"message":{"content":"hi"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":4,"completion_tokens":2}}"#,
        );
        let response = provider().complete(&request(None)).expect("turn");
        assert_eq!(response.text, "hi");
        assert_eq!(
            response.usage,
            Usage {
                input_tokens: 4,
                output_tokens: 2
            }
        );
        assert_eq!(response.retries, 0);
        assert_eq!(call_count(&calls), 1);
        let call = calls.lock().unwrap_or_else(|e| e.into_inner())[0].clone();
        assert!(call.starts_with("https://provider.invalid/v1/chat/completions "), "{call}");
        clear_http_transport();
    }

    #[test]
    fn a_non_idempotent_request_is_not_retried() {
        let _guard = TRANSPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_http_transport();
        let calls = ScriptedTransport::install(503, "unavailable");
        let error = provider().complete(&request(None)).expect_err("http error");
        assert!(matches!(error, ProviderError::Http { status: 503, .. }));
        assert_eq!(call_count(&calls), 1, "an unseeded request must not be replayed");
        clear_http_transport();
    }

    #[test]
    fn a_seeded_request_retries_up_to_the_bound() {
        let _guard = TRANSPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_http_transport();
        let calls = ScriptedTransport::install(500, "boom");
        let error = provider().complete(&request(Some(3))).expect_err("http error");
        assert!(matches!(error, ProviderError::Http { status: 500, .. }));
        assert_eq!(call_count(&calls), 1 + MAX_RETRIES as usize);
        clear_http_transport();
    }

    #[test]
    fn a_missing_transport_fails_closed() {
        let _guard = TRANSPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_http_transport();
        let error = provider().complete(&request(None)).expect_err("no transport");
        assert!(matches!(error, ProviderError::Transport(_)));
        assert!(!provider().honors_seed());
    }

    #[test]
    fn parses_a_tool_call_response_without_text_content() {
        let _guard = TRANSPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_http_transport();
        let calls = ScriptedTransport::install(
            200,
            r#"{"choices":[{"message":{"content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}]},
                "finish_reason":"tool_calls"}]}"#,
        );
        let response = provider().complete(&request(None)).expect("turn");
        assert_eq!(response.text, "");
        assert_eq!(
            response.tool_calls,
            vec![ToolCall {
                name: "read_file".to_string(),
                arguments: r#"{"path":"a.txt"}"#.to_string(),
            }]
        );
        assert_eq!(
            response.finish,
            FinishReason::Other("tool_calls".to_string())
        );
        assert_eq!(call_count(&calls), 1);
        clear_http_transport();
    }

    #[test]
    fn a_message_with_neither_content_nor_tool_calls_is_invalid() {
        let _guard = TRANSPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_http_transport();
        ScriptedTransport::install(200, r#"{"choices":[{"message":{"role":"assistant"}}]}"#);
        let error = provider().complete(&request(None)).expect_err("invalid");
        assert!(matches!(error, ProviderError::InvalidResponse(_)));
        clear_http_transport();
    }

    #[test]
    fn tool_definitions_reach_the_request_body() {
        let request = request(None);
        assert!(provider().request_body(&request).get("tools").is_none());

        let mut with_tools = request;
        with_tools.tools = vec![crate::provider::ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: r#"{"type":"object","properties":{"path":{"type":"string"}}}"#.to_string(),
        }];
        let body = provider().request_body(&with_tools);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
        assert_eq!(body["tools"][0]["function"]["description"], "Read a file");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn json_schema_and_seed_reach_the_request_body() {
        let _guard = TRANSPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut request = request(Some(9));
        request.json_schema = Some(r#"{"type":"object","required":["count"]}"#.to_string());
        let body = provider().request_body(&request);
        assert_eq!(body["seed"], 9);
        assert_eq!(
            body["response_format"]["json_schema"]["schema"]["required"][0],
            "count"
        );
        assert!(body.get("top_k").is_none());
    }
}
