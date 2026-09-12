//! `AgentSpec` parsing and validation (R-3211 T1).
//!
//! The host ABI has no record-value channel, so the plan's `AgentSpec` record
//! crosses as a JSON document (adaptation 11). Parsing is strict: unknown keys
//! are rejected rather than ignored, because a typo in a capability ceiling is
//! exactly the failure mode a governance layer must not hide.

use serde_json::Value;

use crate::error::AgentError;

/// Taint handling requested by the spec. Enforcement lands with R-3223; the
/// value is validated and stored so a run cannot silently lose the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UntrustedPolicy {
    Approve,
    Block,
    Allow,
}

impl UntrustedPolicy {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "approve" => Self::Approve,
            "block" => Self::Block,
            "allow" => Self::Allow,
            _ => return None,
        })
    }
}

/// Validated `AgentSpec`. Budget ceilings (`max_tokens`, `max_cost_micros`,
/// `max_seconds`, `max_tool_calls`) are parsed and stored; enforcing them is
/// R-3216, so this version only forwards `max_tokens` to the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSpec {
    pub goal: String,
    pub model: String,
    pub endpoint: String,
    pub allow: Vec<String>,
    pub max_tokens: i64,
    pub max_cost_micros: i64,
    pub max_seconds: i64,
    pub max_tool_calls: i64,
    pub untrusted: UntrustedPolicy,
    pub seed: i64,
    /// Journal directory. Absent means [`crate::journal::DEFAULT_DIR`]; an
    /// explicit empty string disables the journal (R-3217 T1).
    pub journal: String,
    /// Whether request payloads (prompts, tool arguments) are captured.
    /// Digests are always recorded; payload capture is opt-in (R-3217).
    pub journal_payloads: bool,
    /// Run identity to resume. Empty means a fresh run id.
    pub run_id: String,
}

impl AgentSpec {
    /// Whether this run asked for deterministic sampling.
    pub(crate) fn deterministic(&self) -> bool {
        self.seed >= 0
    }

    /// Provider requests are reconstructed from the spec, not from the run
    /// state, so an in-flight turn never observes a mutated spec.
    pub(crate) fn parse(json: &str) -> Result<Self, AgentError> {
        let value: Value = serde_json::from_str(json)
            .map_err(|error| AgentError::InvalidSpec(format!("spec is not valid JSON: {error}")))?;
        let object = value
            .as_object()
            .ok_or_else(|| AgentError::InvalidSpec("spec must be a JSON object".to_string()))?;

        for key in object.keys() {
            if !KNOWN_KEYS.contains(&key.as_str()) {
                return Err(AgentError::InvalidSpec(format!(
                    "unknown spec field '{key}' (known: {})",
                    KNOWN_KEYS.join(", ")
                )));
            }
        }

        let goal = required_string(object, "goal")?;
        let model = required_string(object, "model")?;
        let endpoint = optional_string(object, "endpoint")?;
        let allow = optional_string_list(object, "allow")?;
        let max_tokens = optional_non_negative(object, "max_tokens")?;
        let max_cost_micros = optional_non_negative(object, "max_cost_micros")?;
        let max_seconds = optional_non_negative(object, "max_seconds")?;
        let max_tool_calls = optional_non_negative(object, "max_tool_calls")?;
        // `journal` is optional with a default directory; an explicit empty
        // string is the documented "no journal" spelling, so a pre-R-3217 spec
        // that passes `""` stays journal-free.
        let journal = match object.get("journal") {
            None => crate::journal::DEFAULT_DIR.to_string(),
            Some(Value::String(value)) => value.clone(),
            Some(_) => {
                return Err(AgentError::InvalidSpec(
                    "spec field 'journal' must be a string".to_string(),
                ))
            }
        };
        let journal_payloads = optional_bool(object, "journal_payloads")?;
        let run_id = optional_string(object, "run_id")?;

        let untrusted = match object.get("untrusted") {
            None => UntrustedPolicy::Approve,
            Some(Value::String(raw)) => UntrustedPolicy::parse(raw).ok_or_else(|| {
                AgentError::InvalidSpec(format!(
                    "spec field 'untrusted' must be one of approve|block|allow, found '{raw}'"
                ))
            })?,
            Some(_) => {
                return Err(AgentError::InvalidSpec(
                    "spec field 'untrusted' must be a string".to_string(),
                ))
            }
        };

        let seed = match object.get("seed") {
            None => -1,
            Some(Value::Number(number)) => number.as_i64().ok_or_else(|| {
                AgentError::InvalidSpec("spec field 'seed' must be an integer".to_string())
            })?,
            Some(_) => {
                return Err(AgentError::InvalidSpec(
                    "spec field 'seed' must be an integer".to_string(),
                ))
            }
        };
        if seed < -1 {
            return Err(AgentError::InvalidSpec(format!(
                "spec field 'seed' must be -1 (provider default) or >= 0, found {seed}"
            )));
        }

        Ok(Self {
            goal,
            model,
            endpoint,
            allow,
            max_tokens,
            max_cost_micros,
            max_seconds,
            max_tool_calls,
            untrusted,
            seed,
            journal,
            journal_payloads,
            run_id,
        })
    }
}

const KNOWN_KEYS: &[&str] = &[
    "goal",
    "model",
    "endpoint",
    "allow",
    "max_tokens",
    "max_cost_micros",
    "max_seconds",
    "max_tool_calls",
    "untrusted",
    "seed",
    "journal",
    "journal_payloads",
    "run_id",
];

fn required_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, AgentError> {
    match object.get(key) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        Some(Value::String(_)) => Err(AgentError::InvalidSpec(format!(
            "spec field '{key}' must not be empty"
        ))),
        Some(_) => Err(AgentError::InvalidSpec(format!(
            "spec field '{key}' must be a string"
        ))),
        None => Err(AgentError::InvalidSpec(format!(
            "spec field '{key}' is required"
        ))),
    }
}

fn optional_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, AgentError> {
    match object.get(key) {
        None => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(AgentError::InvalidSpec(format!(
            "spec field '{key}' must be a string"
        ))),
    }
}

fn optional_string_list(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, AgentError> {
    match object.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(values)) => {
            let mut grants = Vec::with_capacity(values.len());
            for value in values {
                let Value::String(grant) = value else {
                    return Err(AgentError::InvalidSpec(format!(
                        "spec field '{key}' must be an array of strings"
                    )));
                };
                if grant.trim().is_empty() {
                    return Err(AgentError::InvalidSpec(format!(
                        "spec field '{key}' contains an empty capability"
                    )));
                }
                grants.push(grant.clone());
            }
            Ok(grants)
        }
        Some(_) => Err(AgentError::InvalidSpec(format!(
            "spec field '{key}' must be an array of strings"
        ))),
    }
}

fn optional_non_negative(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<i64, AgentError> {
    match object.get(key) {
        None => Ok(0),
        Some(Value::Number(number)) => {
            let value = number.as_i64().ok_or_else(|| {
                AgentError::InvalidSpec(format!("spec field '{key}' must be an integer"))
            })?;
            if value < 0 {
                return Err(AgentError::InvalidSpec(format!(
                    "spec field '{key}' must be >= 0, found {value}"
                )));
            }
            Ok(value)
        }
        Some(_) => Err(AgentError::InvalidSpec(format!(
            "spec field '{key}' must be an integer"
        ))),
    }
}

fn optional_bool(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<bool, AgentError> {
    match object.get(key) {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(AgentError::InvalidSpec(format!(
            "spec field '{key}' must be a boolean"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_spec() -> String {
        r#"{
            "goal": "summarize",
            "model": "mock/echo",
            "endpoint": "mock:",
            "allow": ["spectra.std.fs.read"],
            "max_tokens": 128,
            "max_cost_micros": 500,
            "max_seconds": 30,
            "max_tool_calls": 4,
            "untrusted": "block",
            "seed": 7,
            "journal": "target/journal"
        }"#
        .to_string()
    }

    #[test]
    fn minimal_spec_uses_documented_defaults() {
        let spec = AgentSpec::parse(r#"{"goal":"g","model":"mock/echo"}"#).expect("valid spec");
        assert_eq!(spec.endpoint, "");
        assert!(spec.allow.is_empty());
        assert_eq!(spec.seed, -1);
        assert!(!spec.deterministic());
        assert_eq!(spec.untrusted, UntrustedPolicy::Approve);
        assert_eq!(spec.max_tokens, 0);
    }

    #[test]
    fn full_spec_round_trips_every_field() {
        let spec = AgentSpec::parse(&full_spec()).expect("valid spec");
        assert_eq!(spec.goal, "summarize");
        assert_eq!(spec.allow, vec!["spectra.std.fs.read".to_string()]);
        assert_eq!(spec.untrusted, UntrustedPolicy::Block);
        assert_eq!(spec.seed, 7);
        assert!(spec.deterministic());
        assert_eq!(spec.journal, "target/journal");
    }

    #[test]
    fn journal_defaults_to_the_standard_directory_and_can_be_disabled() {
        let spec = AgentSpec::parse(r#"{"goal":"g","model":"mock/echo"}"#).expect("valid spec");
        assert_eq!(spec.journal, crate::journal::DEFAULT_DIR);
        assert!(!spec.journal_payloads);
        assert_eq!(spec.run_id, "");
        let disabled =
            AgentSpec::parse(r#"{"goal":"g","model":"mock/echo","journal":""}"#).expect("valid");
        assert_eq!(disabled.journal, "");
        let resumed = AgentSpec::parse(
            r#"{"goal":"g","model":"mock/echo","run_id":"run-1","journal":"target/j","journal_payloads":true}"#,
        )
        .expect("valid");
        assert_eq!(resumed.run_id, "run-1");
        assert_eq!(resumed.journal, "target/j");
        assert!(resumed.journal_payloads);
        let error = AgentSpec::parse(r#"{"goal":"g","model":"m","journal_payloads":"yes"}"#)
            .expect_err("must reject a non-boolean");
        assert!(error.detail().contains("journal_payloads"), "{error:?}");
    }

    #[test]
    fn invalid_specs_fail_closed_with_a_named_field() {
        for (json, needle) in [
            (r#"{}"#, "goal"),
            (r#"{"goal":"g"}"#, "model"),
            (r#"{"goal":"","model":"m"}"#, "must not be empty"),
            (r#"{"goal":"g","model":"m","seed":-2}"#, "seed"),
            (r#"{"goal":"g","model":"m","untrusted":"maybe"}"#, "untrusted"),
            (r#"{"goal":"g","model":"m","max_tokens":-1}"#, "max_tokens"),
            (r#"{"goal":"g","model":"m","allow":[""]}"#, "empty capability"),
            (r#"{"goal":"g","model":"m","max_token":5}"#, "unknown spec field"),
            ("not json", "not valid JSON"),
        ] {
            let error = AgentSpec::parse(json).expect_err(json);
            assert!(
                error.detail().contains(needle),
                "expected {needle:?} in {error:?}"
            );
        }
    }
}
