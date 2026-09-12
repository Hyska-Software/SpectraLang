//! Deterministic mock provider.
//!
//! Mandatory for R-3211: every agent acceptance path must run without a
//! network. The mock is selected by `endpoint: "mock:"` or a `mock/` model
//! prefix. Its text, usage, chunks and embedding are pure functions of the
//! prompt, so JIT and AOT runs report identical counters.
//!
//! Scripted mode: a prompt containing `spectra:invalid-json` returns a
//! document that violates an integer-typed schema (used to prove client-side
//! `ask_json` validation), and `spectra:json` returns a schema-shaped
//! document. A prompt containing `spectra:sleep-ms=N` sleeps N milliseconds
//! before answering, which gives the wall-time ceiling (R-3216) a
//! deterministic slow call. Every other prompt echoes.
//!
//! Tool scripting (R-3222): directive lines in the first user message drive a
//! model/tool loop without a network. `spectra:tool=<name> <json-arguments>`
//! answers the turn with exactly one tool call and no text; `spectra:final=<text>`
//! ends the loop with text and no tool calls. The directive answered is the
//! one at index `N`, where `N` is the number of `"tool"` messages already in
//! the request, so a script is consumed one turn at a time.

use std::time::Duration;

use spectra_runtime::stdlib::text_token_count;

use super::{
    FinishReason, Provider, ProviderError, ProviderRequest, ProviderResponse, ProviderStream,
    ToolCall, Usage,
};

/// Cost table of the mock provider, in micros per token.
pub(crate) const COST_MICROS_PER_INPUT_TOKEN: u64 = 1;
pub(crate) const COST_MICROS_PER_OUTPUT_TOKEN: u64 = 2;

/// Fixed embedding width of the mock provider.
pub(crate) const EMBEDDING_DIM: usize = 8;

/// Chunk size, in characters, used by the deterministic stream.
const STREAM_CHUNK_CHARS: usize = 12;

/// Scripted prompt marker that makes the next call slow.
const SLEEP_MARKER: &str = "spectra:sleep-ms=";

/// Scripted directive that asks for a tool call on turn `N`.
const TOOL_DIRECTIVE: &str = "spectra:tool=";

/// Scripted directive that ends the script with final text.
const FINAL_DIRECTIVE: &str = "spectra:final=";

/// Upper bound for the scripted sleep, so a typo cannot park a worker for
/// minutes.
const MAX_SLEEP_MS: u64 = 60_000;

/// Reads the requested sleep from a prompt, if the marker is present.
pub(crate) fn requested_sleep_ms(prompt: &str) -> Option<u64> {
    let start = prompt.find(SLEEP_MARKER)? + SLEEP_MARKER.len();
    let digits: String = prompt[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    Some(digits.parse::<u64>().unwrap_or(MAX_SLEEP_MS).min(MAX_SLEEP_MS))
}

/// One scripted directive line.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Directive {
    /// Call the named tool with these raw JSON arguments.
    Tool { name: String, arguments: String },
    /// End the script with this text and no tool calls.
    Final(String),
}

/// Parses one directive line, if it is one.
///
/// `spectra:tool=<name> <json>` names the tool and takes the rest of the line
/// (after the first space following the name) as the raw arguments; an empty
/// remainder means `{}`. `spectra:final=<text>` ends the script.
fn parse_directive(line: &str) -> Option<Directive> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix(TOOL_DIRECTIVE) {
        let (name, arguments) = match rest.find(char::is_whitespace) {
            Some(index) => (rest[..index].to_string(), rest[index..].trim().to_string()),
            None => (rest.to_string(), String::new()),
        };
        return Some(Directive::Tool {
            name,
            arguments: if arguments.is_empty() {
                "{}".to_string()
            } else {
                arguments
            },
        });
    }
    line.strip_prefix(FINAL_DIRECTIVE)
        .map(|text| Directive::Final(text.trim().to_string()))
}

pub(crate) struct MockProvider {
    model: String,
}

impl MockProvider {
    pub(crate) fn new(model: String) -> Self {
        Self { model }
    }

    /// The prompt of the last user message.
    fn last_prompt(request: &ProviderRequest) -> &str {
        request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.as_str())
            .unwrap_or("")
    }

    /// The first user message, which carries the scripted directives.
    fn first_user_prompt(request: &ProviderRequest) -> &str {
        request
            .messages
            .iter()
            .find(|message| message.role == "user")
            .map(|message| message.content.as_str())
            .unwrap_or("")
    }

    /// The scripted directives of the first user message, in order.
    fn directives(request: &ProviderRequest) -> Vec<Directive> {
        Self::first_user_prompt(request)
            .lines()
            .filter_map(parse_directive)
            .collect()
    }

    /// The scripted response for this turn, if the request carries directives.
    ///
    /// The turn index is the number of completed tool results in the request,
    /// so turn `N` answers directive `N`. When the script is exhausted, a
    /// trailing `spectra:final=` is replayed and anything else stops with no
    /// tool call.
    fn scripted_response(request: &ProviderRequest) -> Option<(String, Vec<ToolCall>)> {
        let directives = Self::directives(request);
        let completed = request
            .messages
            .iter()
            .filter(|message| message.role == "tool")
            .count();
        let directive = match directives.get(completed) {
            Some(directive) => directive,
            None => match directives.last()? {
                Directive::Final(text) => return Some((text.clone(), Vec::new())),
                Directive::Tool { .. } => return Some((String::new(), Vec::new())),
            },
        };
        Some(match directive {
            Directive::Tool { name, arguments } => (
                String::new(),
                vec![ToolCall {
                    name: name.clone(),
                    arguments: arguments.clone(),
                }],
            ),
            Directive::Final(text) => (text.clone(), Vec::new()),
        })
    }

    fn response_text(&self, prompt: &str) -> String {
        if prompt.contains("spectra:invalid-json") {
            // `count` is a string where the schema requires an integer.
            r#"{"count":"not-a-number","label":"mock"}"#.to_string()
        } else if prompt.contains("spectra:json") {
            r#"{"count":3,"label":"mock-response"}"#.to_string()
        } else if self.model == "mock/echo" || self.model.is_empty() {
            format!("mock echo: {prompt}")
        } else {
            format!("mock({}) echo: {prompt}", self.model)
        }
    }
}

impl Provider for MockProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn honors_seed(&self) -> bool {
        // The mock's output is seed-independent and therefore trivially
        // honors any requested seed: the same request always yields the same
        // response.
        true
    }

    fn reports_cost(&self) -> bool {
        // Every mock response carries its deterministic, price-table-derived
        // cost, so a `max_cost_micros` ceiling is enforceable.
        true
    }

    fn complete(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let prompt = Self::last_prompt(request);
        if let Some(milliseconds) = requested_sleep_ms(prompt) {
            std::thread::sleep(Duration::from_millis(milliseconds));
        }
        let (text, tool_calls) = match Self::scripted_response(request) {
            Some(response) => response,
            None => (self.response_text(prompt), Vec::new()),
        };
        let input_tokens = text_token_count(prompt) as u64;
        let output_tokens = text_token_count(&text) as u64;
        Ok(ProviderResponse {
            text,
            tool_calls,
            usage: Usage {
                input_tokens,
                output_tokens,
            },
            cost_micros: input_tokens * COST_MICROS_PER_INPUT_TOKEN
                + output_tokens * COST_MICROS_PER_OUTPUT_TOKEN,
            retries: 0,
            finish: FinishReason::Stop,
        })
    }

    fn stream(&self, request: &ProviderRequest) -> Result<ProviderStream, ProviderError> {
        let response = self.complete(request)?;
        let mut chunks = Vec::new();
        let mut current = String::new();
        let mut width = 0usize;
        for character in response.text.chars() {
            current.push(character);
            width += 1;
            if width >= STREAM_CHUNK_CHARS {
                chunks.push(std::mem::take(&mut current));
                width = 0;
            }
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        // An empty response produces no chunk; the host still answers `""` for
        // the first `stream_next`, which ends the stream.
        Ok(ProviderStream {
            chunks,
            usage: response.usage,
            cost_micros: response.cost_micros,
            retries: response.retries,
        })
    }

    fn embed(&self, text: &str) -> Result<Vec<f64>, ProviderError> {
        // Deterministic FNV-1a-derived vector in [-1, 1).
        let mut vector = Vec::with_capacity(EMBEDDING_DIM);
        for index in 0..EMBEDDING_DIM {
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            for byte in text.as_bytes() {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            hash ^= index as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            vector.push(((hash % 2000) as f64 / 1000.0) - 1.0);
        }
        Ok(vector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Message;

    fn request(prompt: &str, seed: Option<i64>) -> ProviderRequest {
        ProviderRequest {
            model: "mock/echo".to_string(),
            messages: vec![Message::user(prompt)],
            temperature: seed.map(|_| 0.0),
            top_k: None,
            seed,
            json_schema: None,
            max_tokens: None,
            tools: Vec::new(),
        }
    }

    #[test]
    fn text_usage_and_cost_are_deterministic() {
        let provider = MockProvider::new("mock/echo".to_string());
        let first = provider.complete(&request("alpha", Some(1))).expect("turn");
        let second = provider.complete(&request("alpha", Some(1))).expect("turn");
        assert_eq!(first, second);
        assert_eq!(first.text, "mock echo: alpha");
        // 1 prompt token, 3 response tokens (mock, echo, alpha).
        assert_eq!(first.usage.input_tokens, 1);
        assert_eq!(first.usage.output_tokens, 3);
        assert_eq!(first.cost_micros, 1 + 3 * COST_MICROS_PER_OUTPUT_TOKEN);
        assert!(provider.honors_seed());
    }

    #[test]
    fn streaming_is_chunked_and_never_emits_an_empty_chunk() {
        let provider = MockProvider::new("mock/echo".to_string());
        let stream = provider.stream(&request("alpha", None)).expect("stream");
        assert!(stream.chunks.len() >= 2, "{:?}", stream.chunks);
        assert!(stream.chunks.iter().all(|chunk| !chunk.is_empty()));
        assert_eq!(stream.chunks.concat(), "mock echo: alpha");
        assert_eq!(stream.usage.input_tokens, 1);
        assert!(stream.cost_micros > 0);
    }

    #[test]
    fn scripted_prompts_drive_the_json_paths() {
        let provider = MockProvider::new("mock/echo".to_string());
        assert_eq!(
            provider.response_text("please spectra:json"),
            r#"{"count":3,"label":"mock-response"}"#
        );
        assert_eq!(
            provider.response_text("please spectra:invalid-json"),
            r#"{"count":"not-a-number","label":"mock"}"#
        );
    }

    #[test]
    fn scripted_directives_drive_a_two_tool_then_final_loop() {
        let provider = MockProvider::new("mock/echo".to_string());
        let script = concat!(
            "do the thing\n",
            "spectra:tool=alpha {\"path\":\"a.txt\"}\n",
            "spectra:tool=beta\n",
            "spectra:final=done",
        );
        let scripted = |tool_results: usize| {
            let mut request = request(script, None);
            for index in 0..tool_results {
                request.messages.push(Message::assistant(""));
                request
                    .messages
                    .push(Message::tool("ignored-name", format!("result {index}")));
            }
            request
        };

        let first = provider.complete(&scripted(0)).expect("turn 1");
        assert_eq!(first.text, "");
        assert_eq!(
            first.tool_calls,
            vec![ToolCall {
                name: "alpha".to_string(),
                arguments: r#"{"path":"a.txt"}"#.to_string(),
            }]
        );

        let second = provider.complete(&scripted(1)).expect("turn 2");
        assert_eq!(second.text, "");
        // A directive with no arguments payload means an empty JSON object.
        assert_eq!(
            second.tool_calls,
            vec![ToolCall {
                name: "beta".to_string(),
                arguments: "{}".to_string(),
            }]
        );

        let third = provider.complete(&scripted(2)).expect("turn 3");
        assert_eq!(third.text, "done");
        assert!(third.tool_calls.is_empty());

        // Past the end of the script, a trailing final text is replayed.
        let replayed = provider.complete(&scripted(5)).expect("turn 4");
        assert_eq!(replayed.text, "done");
        assert!(replayed.tool_calls.is_empty());

        // A script that ends on a tool directive stops instead of replaying it.
        let mut tool_only = request("spectra:tool=alpha", None);
        tool_only.messages.push(Message::tool("ignored-name", "result"));
        let stopped = provider.complete(&tool_only).expect("turn");
        assert_eq!(stopped.text, "");
        assert!(stopped.tool_calls.is_empty());
    }

    #[test]
    fn embeddings_are_deterministic_and_text_dependent() {
        let provider = MockProvider::new("mock/echo".to_string());
        let first = provider.embed("alpha").expect("embed");
        assert_eq!(first.len(), EMBEDDING_DIM);
        assert_eq!(first, provider.embed("alpha").expect("embed"));
        assert_ne!(first, provider.embed("beta").expect("embed"));
        assert!(first.iter().all(|value| (-1.0..1.0).contains(value)));
    }
}
