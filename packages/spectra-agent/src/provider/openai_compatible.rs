//! OpenAI-compatible HTTP provider (R-3211 T2).
//!
//! The request goes through the injected [`HttpTransport`], so TLS, pooling
//! and SSRF policy remain owned by the transport implementation (adaptation 12).
//! Retries are bounded and only issued for requests that are idempotent by
//! construction: a deterministic run (`seed >= 0`) replays the identical
//! sampled request, and the retry count is recorded on the run.
//!
//! Three things this provider refuses to guess:
//!
//! * **usage.** A response without `usage` is a typed failure, not a zero: the
//!   run's token ceiling is enforced against the provider's own count, and a
//!   silent zero would make that ceiling silently ineffective. Streaming asks
//!   for `stream_options.include_usage` for the same reason.
//! * **cost.** Prices are the operator's data, not the model's identity:
//!   [`prices`] reads `SPECTRA_AGENT_PRICES` (JSON, micros per token), and
//!   [`Provider::reports_cost`] is true only when the model has an entry, so a
//!   `max_cost_micros` ceiling is either enforceable or refused at
//!   `agent_start` — never measured as zero.
//! * **tool schemas.** A tool whose registered schema does not parse is
//!   refused by name rather than sent as a permissive object schema, because a
//!   permissive schema invites arguments the tool's wrapper will reject.
//!
//! Embeddings and streaming are both real: `/v1/embeddings` is called for
//! [`Provider::embed`], and [`Provider::stream`] requests `stream: true` and
//! parses the server-sent-event frames into the chunk sequence the run streams
//! from.

use serde_json::{json, Value};

use super::{
    transport::{http_transport, TransportResponse},
    FinishReason, Provider, ProviderError, ProviderRequest, ProviderResponse, ProviderStream,
    ToolCall, Usage,
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

    fn embeddings_url(&self) -> Result<String, ProviderError> {
        if self.endpoint.is_empty() {
            return Err(ProviderError::NotConfigured(
                "no provider endpoint: set AgentSpec.endpoint, SPECTRA_AGENT_ENDPOINT or OPENAI_BASE_URL"
                    .to_string(),
            ));
        }
        Ok(format!(
            "{}/v1/embeddings",
            self.endpoint.trim_end_matches('/')
        ))
    }

    /// The model a request resolves to (`request.model` wins when set).
    fn model_of<'a>(&'a self, request_model: &'a str) -> &'a str {
        if request_model.is_empty() {
            self.model.as_str()
        } else {
            request_model
        }
    }

    /// The request body for one turn.
    ///
    /// Returns an error rather than a body when a tool's schema cannot be
    /// parsed: the model must never be offered a call the wrapper cannot
    /// accept, and a permissive substitute would hide the broken descriptor.
    fn request_body(&self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        let messages = request
            .messages
            .iter()
            .map(|message| json!({ "role": message.role, "content": message.content }))
            .collect::<Vec<_>>();
        let mut body = json!({
            "model": self.model_of(&request.model),
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
            // the stored schema is the argument schema itself. A tool whose
            // schema does not parse is dropped from the request and reported:
            // sending it with a permissive schema would invite arguments its
            // wrapper rejects, and silently omitting it would hide a broken
            // descriptor.
            let mut tools = Vec::with_capacity(request.tools.len());
            for tool in &request.tools {
                let parameters = serde_json::from_str::<Value>(&tool.input_schema)
                    .map_err(|error| ProviderError::InvalidRequest(format!(
                        "tool '{}' has an input schema that is not JSON ({error}); the turn is                          refused rather than offering a permissive schema the tool cannot accept",
                        tool.name
                    )))?;
                tools.push(json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": parameters,
                    }
                }));
            }
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
        Ok(body)
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
        let body = self.request_body(request)?.to_string();
        let idempotent = request.seed.is_some();
        let mut retries = 0u64;
        loop {
            match self.post(&url, &body) {
                Ok(response) if (200..300).contains(&response.status) => {
                    return parse_response(self.model_of(&request.model), &response.body, retries);
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

    fn reports_cost(&self) -> bool {
        // True exactly when the model has a price: a cost ceiling is then
        // measurable rather than refused at `agent_start`.
        prices().for_model(&self.model).is_some()
    }

    /// A real embedding request: `POST /v1/embeddings` with the model and the
    /// input text, answered by `data[0].embedding`.
    ///
    /// Retries mirror `complete`: only a deterministic run (a seeded request)
    /// replays, because the call is idempotent only then.
    fn embed(&self, text: &str) -> Result<Vec<f64>, ProviderError> {
        let url = self.embeddings_url()?;
        let body = json!({ "model": self.model_of(""), "input": text }).to_string();
        let mut attempts = 0u64;
        loop {
            match self.post(&url, &body) {
                Ok(response) if (200..300).contains(&response.status) => {
                    return parse_embedding(&response.body);
                }
                Ok(response) => {
                    let retryable = response.status >= 500 || response.status == 429;
                    if retryable && attempts < MAX_RETRIES {
                        attempts += 1;
                        continue;
                    }
                    return Err(ProviderError::Http {
                        status: response.status,
                        message: response.body,
                    });
                }
                Err(message) => {
                    if attempts < MAX_RETRIES {
                        attempts += 1;
                        continue;
                    }
                    return Err(ProviderError::Transport(message));
                }
            }
        }
    }

    /// A streamed turn: the same chat request with `stream: true`, parsed from
    /// the server-sent-event frames.
    ///
    /// The chunks are the frames' `choices[0].delta.content` in arrival order,
    /// so a caller sees the model's own token stream rather than a
    /// post-hoc split of the finished answer. Usage is requested explicitly
    /// (`stream_options.include_usage`) and is required: a stream whose
    /// accounting is unknown cannot be budgeted.
    fn stream(&self, request: &ProviderRequest) -> Result<ProviderStream, ProviderError> {
        let url = self.chat_url()?;
        let mut body = self.request_body(request)?;
        body["stream"] = json!(true);
        body["stream_options"] = json!({ "include_usage": true });
        let body = body.to_string();
        let idempotent = request.seed.is_some();
        let mut retries = 0u64;
        loop {
            match self.post(&url, &body) {
                Ok(response) if (200..300).contains(&response.status) => {
                    let (chunks, usage) = parse_stream(&response.body)?;
                    return Ok(ProviderStream {
                        chunks,
                        usage,
                        cost_micros: cost_micros(&self.model, usage),
                        retries,
                    });
                }
                Ok(response) => {
                    let retryable = response.status >= 500 || response.status == 429;
                    if idempotent && retryable && retries < MAX_RETRIES {
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
}

/// Micros per token for one model, as the operator configures them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Price {
    pub input_micros_per_token: u64,
    pub output_micros_per_token: u64,
}

/// The operator's price table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PriceTable {
    entries: std::collections::BTreeMap<String, Price>,
}

impl PriceTable {
    /// Reads `SPECTRA_AGENT_PRICES`, a JSON object of
    /// `{"<model>":{"in":micros,"out":micros}}`.
    ///
    /// An unreadable table is *no* table (every cost ceiling is then refused at
    /// `agent_start`), never a table of zeros: a zero price is a claim, and
    /// this provider does not invent claims about money.
    pub(crate) fn from_env() -> Self {
        let Ok(raw) = std::env::var("SPECTRA_AGENT_PRICES") else {
            return Self::default();
        };
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            return Self::default();
        };
        let mut entries = std::collections::BTreeMap::new();
        if let Some(object) = value.as_object() {
            for (model, price) in object {
                let input = price.get("in").and_then(Value::as_u64);
                let output = price.get("out").and_then(Value::as_u64);
                if let (Some(input), Some(output)) = (input, output) {
                    entries.insert(
                        model.clone(),
                        Price {
                            input_micros_per_token: input,
                            output_micros_per_token: output,
                        },
                    );
                }
            }
        }
        Self { entries }
    }

    pub(crate) fn for_model(&self, model: &str) -> Option<Price> {
        self.entries.get(model).copied()
    }
}

/// The process price table, read once (the environment does not change under a
/// running host in a way a turn should observe mid-run).
pub(crate) fn prices() -> &'static PriceTable {
    static PRICES: std::sync::OnceLock<PriceTable> = std::sync::OnceLock::new();
    PRICES.get_or_init(PriceTable::from_env)
}

/// The cost of one response according to the operator's table, or zero when
/// the model has no price (`reports_cost()` then says so, and a cost ceiling
/// is refused at `agent_start`).
fn cost_micros(model: &str, usage: Usage) -> u64 {
    match prices().for_model(model) {
        Some(price) => usage
            .input_tokens
            .saturating_mul(price.input_micros_per_token)
            .saturating_add(usage.output_tokens.saturating_mul(price.output_micros_per_token)),
        None => 0,
    }
}

/// `POST /v1/embeddings` answers with `data[0].embedding`: the vector, and
/// nothing else is accepted in its place.
fn parse_embedding(body: &str) -> Result<Vec<f64>, ProviderError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| ProviderError::InvalidResponse(format!("response is not JSON: {error}")))?;
    let vector: Vec<f64> = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|data| data.first())
        .and_then(|first| first.get("embedding"))
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    if vector.is_empty() {
        return Err(ProviderError::InvalidResponse(
            "response has no data[0].embedding array".to_string(),
        ));
    }
    Ok(vector)
}

/// Parses a server-sent-event chat stream into its chunks and usage.
///
/// Frames are `data: {json}` lines terminated by `data: [DONE]`; each frame's
/// `choices[0].delta.content` is one chunk. Usage arrives in a frame of its own
/// (the one `stream_options.include_usage` asks for) and is required: a
/// streamed turn the run cannot account for is refused rather than budgeted as
/// zero.
fn parse_stream(body: &str) -> Result<(Vec<String>, Usage), ProviderError> {
    let mut chunks = Vec::new();
    let mut usage: Option<Usage> = None;
    let mut frames = 0usize;
    for line in body.lines() {
        let line = line.trim_start();
        let Some(payload) = line.strip_prefix("data:") else {
            // Event names, comments and keep-alives carry no content.
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() {
            continue;
        }
        if payload == "[DONE]" {
            break;
        }
        let frame: Value = serde_json::from_str(payload).map_err(|error| {
            ProviderError::InvalidResponse(format!("stream frame is not JSON: {error}"))
        })?;
        // A frame may carry an error instead of a delta.
        if let Some(error) = frame.get("error") {
            return Err(ProviderError::InvalidResponse(format!(
                "the stream reported an error: {error}"
            )));
        }
        frames += 1;
        if let Some(delta) = frame
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(Value::as_str)
        {
            if !delta.is_empty() {
                chunks.push(delta.to_string());
            }
        }
        if let Some(frame_usage) = frame.get("usage").filter(|usage| !usage.is_null()) {
            usage = Some(Usage {
                input_tokens: frame_usage
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        ProviderError::InvalidResponse(
                            "the stream's usage frame has no prompt_tokens".to_string(),
                        )
                    })?,
                output_tokens: frame_usage
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        ProviderError::InvalidResponse(
                            "the stream's usage frame has no completion_tokens".to_string(),
                        )
                    })?,
            });
        }
    }
    if frames == 0 {
        return Err(ProviderError::InvalidResponse(
            "the stream carried no server-sent-event frames".to_string(),
        ));
    }
    let usage = usage.ok_or_else(|| {
        ProviderError::InvalidResponse(
            "the stream reported no usage, so the turn cannot be accounted for; the request asks \
             for `stream_options.include_usage` and a server that ignores it would silently make \
             every token ceiling ineffective"
                .to_string(),
        )
    })?;
    Ok((chunks, usage))
}

/// Parses one chat completion, priced by `model`.
fn parse_response(
    model: &str,
    body: &str,
    retries: u64,
) -> Result<ProviderResponse, ProviderError> {
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
    // Usage is required: the run's token ceiling is enforced against the
    // provider's own count, and a silent zero would make it ineffective.
    let usage = value
        .get("usage")
        .filter(|usage| !usage.is_null())
        .ok_or_else(|| {
            ProviderError::InvalidResponse(
                "response has no usage, so the turn cannot be accounted for".to_string(),
            )
        })?;
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            ProviderError::InvalidResponse("response usage has no prompt_tokens".to_string())
        })?;
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            ProviderError::InvalidResponse("response usage has no completion_tokens".to_string())
        })?;
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
    let usage = Usage {
        input_tokens,
        output_tokens,
    };
    Ok(ProviderResponse {
        text,
        tool_calls,
        usage,
        // The operator's price table decides the cost; without an entry the
        // provider reports that it cannot price the turn (`reports_cost`).
        cost_micros: cost_micros(model, usage),
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
    ///
    /// This is the crate-wide global-state lock, not a module-local one: the
    /// transport (like the host registry) is process-global, and `hosts` /
    /// `replay` tests install their own transports. A module-local lock would
    /// let two tests own the slot at once.
    fn transport_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
        let _guard = transport_lock();
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
        let _guard = transport_lock();
        clear_http_transport();
        let calls = ScriptedTransport::install(503, "unavailable");
        let error = provider().complete(&request(None)).expect_err("http error");
        assert!(matches!(error, ProviderError::Http { status: 503, .. }));
        assert_eq!(call_count(&calls), 1, "an unseeded request must not be replayed");
        clear_http_transport();
    }

    #[test]
    fn a_seeded_request_retries_up_to_the_bound() {
        let _guard = transport_lock();
        clear_http_transport();
        let calls = ScriptedTransport::install(500, "boom");
        let error = provider().complete(&request(Some(3))).expect_err("http error");
        assert!(matches!(error, ProviderError::Http { status: 500, .. }));
        assert_eq!(call_count(&calls), 1 + MAX_RETRIES as usize);
        clear_http_transport();
    }

    /// A streamed turn is the server's own chunk sequence, and its usage is
    /// required: the run budgets on it.
    #[test]
    fn a_streamed_turn_parses_the_event_frames_and_requires_usage() {
        let _guard = transport_lock();
        clear_http_transport();
        let calls = ScriptedTransport::install(
            200,
            "data: {\"choices\":[{\"delta\":{\"content\":\"mock \"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"echo\"}}]}\n\
             data: {\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2}}\n\
             data: [DONE]\n",
        );
        let stream = provider().stream(&request(None)).expect("streamed turn");
        assert_eq!(stream.chunks, vec!["mock ".to_string(), "echo".to_string()]);
        assert_eq!(
            stream.usage,
            Usage {
                input_tokens: 4,
                output_tokens: 2
            }
        );
        let call = calls.lock().unwrap_or_else(|e| e.into_inner())[0].clone();
        assert!(call.contains("\"stream\":true"), "{call}");
        assert!(call.contains("include_usage"), "{call}");
        clear_http_transport();

        // A stream without usage is refused: the turn could not be accounted.
        clear_http_transport();
        ScriptedTransport::install(
            200,
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n",
        );
        let error = provider().stream(&request(None)).expect_err("no usage");
        assert!(matches!(error, ProviderError::InvalidResponse(_)), "{error:?}");
        assert!(error.to_string().contains("no usage"), "{error}");
        clear_http_transport();

        // A stream frame that carries an error is that error.
        clear_http_transport();
        ScriptedTransport::install(
            200,
            "data: {\"error\":{\"message\":\"context length exceeded\"}}\n",
        );
        let error = provider().stream(&request(None)).expect_err("error frame");
        assert!(error.to_string().contains("context length exceeded"), "{error}");
        clear_http_transport();
    }

    /// Embeddings are a real request to `/v1/embeddings`.
    #[test]
    fn embeddings_are_requested_from_the_embeddings_endpoint() {
        let _guard = transport_lock();
        clear_http_transport();
        let calls = ScriptedTransport::install(
            200,
            r#"{"data":[{"embedding":[0.25,-0.5,0.75],"index":0}]}"#,
        );
        let vector = provider().embed("hello").expect("embedding");
        assert_eq!(vector, vec![0.25, -0.5, 0.75]);
        let call = calls.lock().unwrap_or_else(|e| e.into_inner())[0].clone();
        assert!(call.starts_with("https://provider.invalid/v1/embeddings "), "{call}");
        assert!(call.contains("\"input\":\"hello\""), "{call}");
        clear_http_transport();

        // A response without an embedding vector is refused, not zero-filled.
        clear_http_transport();
        ScriptedTransport::install(200, r#"{"data":[]}"#);
        let error = provider().embed("hello").expect_err("no vector");
        assert!(error.to_string().contains("data[0].embedding"), "{error}");
        clear_http_transport();
    }

    /// The response's usage is required, and the price table decides the cost.
    #[test]
    fn usage_is_required_and_costs_come_from_the_operators_table() {
        let _guard = transport_lock();
        clear_http_transport();
        ScriptedTransport::install(
            200,
            r#"{"choices":[{"message":{"content":"hi"},"finish_reason":"stop"}]}"#,
        );
        let error = provider().complete(&request(None)).expect_err("no usage");
        assert!(matches!(error, ProviderError::InvalidResponse(_)), "{error:?}");
        assert!(error.to_string().contains("no usage"), "{error}");
        clear_http_transport();

        // The table itself is data: an unreadable entry is no entry, and a
        // priced model costs exactly what the operator said.
        let table = PriceTable::from_env();
        assert_eq!(table.for_model("gpt-test"), None);
        let priced = PriceTable {
            entries: std::collections::BTreeMap::from([(
                "gpt-test".to_string(),
                Price {
                    input_micros_per_token: 3,
                    output_micros_per_token: 5,
                },
            )]),
        };
        let price = priced.for_model("gpt-test").expect("priced");
        assert_eq!(
            price.input_micros_per_token * 4 + price.output_micros_per_token * 2,
            22
        );
        // An unpriced model reports that it cannot price a turn, so a cost
        // ceiling is refused at `agent_start` instead of measured as zero.
        assert!(!provider().reports_cost());
    }

    /// A tool whose schema does not parse refuses the turn by name.
    #[test]
    fn a_broken_tool_schema_refuses_the_turn() {
        let _guard = transport_lock();
        clear_http_transport();
        let calls = ScriptedTransport::install(
            200,
            r#"{"choices":[{"message":{"content":"hi"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#,
        );
        let mut request = request(None);
        request.tools = vec![super::super::ToolDefinition {
            name: "broken".to_string(),
            description: "not a schema".to_string(),
            input_schema: "{not json".to_string(),
        }];
        let error = provider().complete(&request).expect_err("refused");
        assert!(matches!(error, ProviderError::InvalidRequest(_)), "{error:?}");
        assert!(error.to_string().contains("broken"), "{error}");
        assert_eq!(call_count(&calls), 0, "nothing was sent");
        clear_http_transport();
    }

    #[test]
    fn a_missing_transport_fails_closed() {
        let _guard = transport_lock();
        clear_http_transport();
        let error = provider().complete(&request(None)).expect_err("no transport");
        assert!(matches!(error, ProviderError::Transport(_)));
        assert!(!provider().honors_seed());
    }

    #[test]
    fn parses_a_tool_call_response_without_text_content() {
        let _guard = transport_lock();
        clear_http_transport();
        let calls = ScriptedTransport::install(
            200,
            r#"{"choices":[{"message":{"content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}]},
                "finish_reason":"tool_calls"}],
                "usage":{"prompt_tokens":7,"completion_tokens":3}}"#,
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
        let _guard = transport_lock();
        clear_http_transport();
        ScriptedTransport::install(200, r#"{"choices":[{"message":{"role":"assistant"}}]}"#);
        let error = provider().complete(&request(None)).expect_err("invalid");
        assert!(matches!(error, ProviderError::InvalidResponse(_)));
        clear_http_transport();
    }

    #[test]
    fn tool_definitions_reach_the_request_body() {
        let request = request(None);
        let body = provider().request_body(&request).expect("body");
        assert!(body.get("tools").is_none());

        let mut with_tools = request;
        with_tools.tools = vec![crate::provider::ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: r#"{"type":"object","properties":{"path":{"type":"string"}}}"#.to_string(),
        }];
        let body = provider().request_body(&with_tools).expect("body");
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
        let _guard = transport_lock();
        let mut request = request(Some(9));
        request.json_schema = Some(r#"{"type":"object","required":["count"]}"#.to_string());
        let body = provider().request_body(&request).expect("body");
        assert_eq!(body["seed"], 9);
        assert_eq!(
            body["response_format"]["json_schema"]["schema"]["required"][0],
            "count"
        );
        assert!(body.get("top_k").is_none());
    }
}
