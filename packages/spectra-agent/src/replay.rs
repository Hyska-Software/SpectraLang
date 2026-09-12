//! Replay algorithm: return recorded effects instead of executing them
//! (R-3217 T2).
//!
//! ADR 0018 fixes the algorithm:
//!
//! 1. Each effect computes its step key — `H(run, step, input digest)` — and
//!    the step index is the run's monotonic effect sequence.
//! 2. A step recorded under the same key returns the recorded output; the
//!    effect is **not** executed.
//! 3. A step with no record executes normally and is appended, so a partial
//!    journal (a crashed run) resumes forward.
//! 4. A step recorded under a *different* key is a divergence: the run would
//!    replay something other than what it is doing, so it fails closed rather
//!    than silently duplicating or skipping an effect.
//!
//! Every effect these helpers wrap goes through [`resolve`] and [`commit`];
//! that pairing is what makes "no effect executes twice across a replay" true
//! rather than aspirational. `commit` flushes before returning, so a crash
//! after an effect cannot lose its record.

use crate::digest;
use crate::error::AgentError;
use crate::journal::{Record, StepUsage};
use crate::run;

/// The kinds of effect the journal records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// One model turn (`chat`), including its tool calls.
    Model,
    /// One embedding request.
    Embed,
    /// One governed tool invocation.
    Tool,
    /// One `approve` decision.
    Approval,
    /// One `require` assertion outcome.
    Assertion,
    /// One provenance or declassification entry (R-3223 T1): the ledger's
    /// audit trail.
    Taint,
    /// One gated sink decision (R-3223 T3): `allow` or `deny` under the run's
    /// `untrusted` policy.
    TaintDecision,
    /// One `compensate` declaration (R-3224 T1): the pending compensation and
    /// the identity a replay uses to rebuild the LIFO stack.
    Compensation,
    /// One executed compensation attempt (R-3224 T2), executed through the
    /// governed dispatch. Recorded even when the compensation fails, so a
    /// replay never re-executes it.
    Rollback,
}

impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Embed => "embed",
            Self::Tool => "tool",
            Self::Approval => "approval",
            Self::Assertion => "assertion",
            Self::Taint => "taint",
            Self::TaintDecision => "taint_decision",
            Self::Compensation => "compensation",
            Self::Rollback => "rollback",
        }
    }
}

/// One resolved step: either a recorded effect to replay, or a fresh step the
/// caller must execute and then [`commit`].
#[derive(Debug, Clone)]
pub(crate) enum Resolved {
    Recorded(Record),
    Fresh(Token),
}

/// Identity of a fresh step, handed back to [`commit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub step: u64,
    pub kind: Kind,
    pub input_digest: String,
}

/// Resolves the next step of `kind` whose input digests to `input`.
///
/// The step number is reserved here, under the run lock, so concurrent effects
/// of one run cannot share a step and the sequence stays monotonic.
pub(crate) fn resolve(
    run_handle: i64,
    kind: Kind,
    input: &str,
) -> Result<Resolved, AgentError> {
    let input_digest = digest::of(input);
    run::with_run(run_handle, |state| -> Result<Resolved, AgentError> {
        let Some(journal) = state.journal.as_mut() else {
            // Journaling is disabled: every step is fresh and nothing is
            // persisted (the pre-R-3217 behavior).
            return Ok(Resolved::Fresh(Token {
                step: 0,
                kind,
                input_digest,
            }));
        };
        let step = journal.next_step;
        journal.next_step += 1;
        match journal.get(step) {
            Some(record) => {
                if record.kind == kind.name() && record.input_digest == input_digest {
                    Ok(Resolved::Recorded(record.clone()))
                } else {
                    Err(AgentError::Journal(format!(
                        "replay divergence at step {step}: the journal holds a '{}' step with \
                         input digest {}, this run is a '{}' step with input digest {}",
                        record.kind,
                        record.input_digest,
                        kind.name(),
                        input_digest
                    )))
                }
            }
            None => Ok(Resolved::Fresh(Token {
                step,
                kind,
                input_digest,
            })),
        }
    })?
}

/// Records one completed effect and flushes it before returning.
///
/// `output` is the effect's result: replay returns it verbatim, so it is
/// always recorded. `input` is the request payload and is stored only when the
/// spec opted into payload capture.
pub(crate) fn commit(
    run_handle: i64,
    token: &Token,
    input: Option<&str>,
    output: &str,
    usage: StepUsage,
    seed: i64,
    attribution: Option<String>,
) -> Result<(), AgentError> {
    run::with_run(run_handle, |state| -> Result<(), AgentError> {
        let run_id = state.run_id.clone();
        let Some(journal) = state.journal.as_mut() else {
            return Ok(());
        };
        let record = Record {
            run: run_id.clone(),
            step: token.step,
            kind: token.kind.name().to_string(),
            input_digest: token.input_digest.clone(),
            output_digest: digest::of(output),
            idempotency_key: digest::step_key(&run_id, token.step, &token.input_digest),
            seed,
            usage,
            timestamp: crate::journal::now_ms(),
            output: output.to_string(),
            input: if journal.captures_payloads() {
                input.map(str::to_string)
            } else {
                None
            },
            attribution,
        };
        journal.append(record)
    })?
}

#[cfg(test)]
mod tests {
    //! I3 proof: a run that crashes mid-flight, resumed with the same run id,
    //! does not execute a completed effect twice. The provider is a counting
    //! test server (an injected `HttpTransport`), so every model call is
    //! counted at the wire, and the tool wrapper counts its own invocations.

    use super::*;
    use crate::provider::transport::{
        clear_http_transport, set_http_transport, HttpTransport, TransportResponse,
    };
    use crate::spec::AgentSpec;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, LazyLock, Mutex};

    /// The tool bodies the counting wrapper executed, in order.
    fn tool_log() -> &'static Mutex<Vec<String>> {
        static LOG: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(Vec::new()));
        &LOG
    }

    /// A wrapper with the ADR 0019 ABI: it doubles `n` and returns the JSON
    /// scalar as its result, recording the arguments it was invoked with.
    extern "C" fn counting_tool(_run: i64, args: i64, out: i64) -> i64 {
        let arguments = crate::abi::read_string_arg(args).unwrap_or_default();
        tool_log()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(arguments.clone());
        let n = serde_json::from_str::<serde_json::Value>(&arguments)
            .ok()
            .and_then(|value| value.get("n").and_then(serde_json::Value::as_i64))
            .unwrap_or(0);
        let result = (n * 2).to_string();
        let pointer = unsafe { crate::abi::alloc_string(&result) };
        unsafe { *(out as *mut i64) = pointer };
        0
    }

    /// A counting OpenAI-compatible server: it scripts the run and can be
    /// armed to fail one call, which is the crash.
    #[derive(Clone)]
    struct CountingServer {
        calls: Arc<AtomicUsize>,
        fail_at: Arc<AtomicUsize>,
    }

    impl CountingServer {
        fn new(fail_at: usize) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                fail_at: Arc::new(AtomicUsize::new(fail_at)),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    fn tool_call_response(name: &str, arguments: &str) -> String {
        serde_json::json!({
            "choices": [{
                "message": {
                    "content": serde_json::Value::Null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": { "name": name, "arguments": arguments }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        })
        .to_string()
    }

    fn final_response(text: &str) -> String {
        serde_json::json!({
            "choices": [{
                "message": { "content": text },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 3 }
        })
        .to_string()
    }

    impl HttpTransport for CountingServer {
        fn post_json(
            &self,
            _url: &str,
            _headers: &[(String, String)],
            body: &str,
        ) -> Result<TransportResponse, String> {
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            if index == self.fail_at.load(Ordering::SeqCst) {
                return Err("simulated crash: the process dies here".to_string());
            }
            // The script is keyed on the tool results already in the
            // transcript, exactly like the deterministic mock.
            let request: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let results = request
                .get("messages")
                .and_then(serde_json::Value::as_array)
                .map(|messages| {
                    messages
                        .iter()
                        .filter(|message| {
                            message.get("role").and_then(serde_json::Value::as_str) == Some("tool")
                        })
                        .count()
                })
                .unwrap_or(0);
            let body = match results {
                0 => tool_call_response("count", r#"{"n":1}"#),
                1 => tool_call_response("count", r#"{"n":2}"#),
                _ => final_response("done"),
            };
            Ok(TransportResponse { status: 200, body })
        }
    }

    fn start(dir: &str, run_id: &str) -> i64 {
        let spec = AgentSpec::parse(&format!(
            r#"{{"goal":"resume-proof","model":"gpt-test","endpoint":"https://provider.invalid","journal":"{}","run_id":"{run_id}"}}"#,
            dir.replace('\\', "/")
        ))
        .expect("valid spec");
        let journal = crate::journal::Journal::open(run_id, dir, false).expect("journal");
        run::alloc_run(spec, run_id.to_string(), Some(journal)).expect("alloc")
    }

    #[test]
    fn a_crashed_run_resumes_without_repeating_a_completed_effect() {
        let _guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // The tool registry is a second process-global resource; its test lock
        // is shared with the registry tests, so neither can clear it while the
        // other holds a registered wrapper.
        let _registry = crate::tools::REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        spectra_runtime::ffi::clear_host_functions();
        spectra_runtime::register();
        crate::register();
        clear_http_transport();
        crate::tools::clear();
        tool_log()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        assert!(crate::tools::register(
            "count".to_string(),
            counting_tool as *const () as usize as i64,
            "doubles n".to_string(),
            r#"{"type":"object"}"#.to_string(),
            "[]",
        ));

        let dir = std::env::temp_dir().join(format!(
            "spectra-replay-{}",
            crate::journal::new_run_id()
        ));
        let dir_text = dir.to_string_lossy().to_string();
        let server = CountingServer::new(2);
        assert!(set_http_transport(server.clone()));

        // Run 1: two model turns and two tool calls complete and are journaled,
        // then the third model call dies.
        let crashed = start(&dir_text, "resume-proof");
        let error = crate::act::act(crashed, "count twice").expect_err("the crash surfaces");
        assert_eq!(error.kind(), "provider_error");
        let state = run::take_run(crashed).expect("end the crashed run");
        assert_eq!(state.status, "failed");
        let journal = state.journal.expect("journal");
        assert_eq!(journal.len(), 4, "four completed effects are durable");
        let kinds: Vec<&str> = journal
            .ordered()
            .map(|record| record.kind.as_str())
            .collect();
        assert_eq!(kinds, ["model", "tool", "model", "tool"]);
        let first_tool_order: Vec<String> = tool_log()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert_eq!(first_tool_order, [r#"{"n":1}"#, r#"{"n":2}"#]);
        let calls_after_crash = server.calls();
        assert_eq!(calls_after_crash, 3, "two successes plus the failing call");

        // Run 2: the same run id resumes the same journal. Nothing recorded is
        // executed again; the run finishes the one effect that never completed.
        server.fail_at.store(usize::MAX, Ordering::SeqCst);
        let resumed = start(&dir_text, "resume-proof");
        let answer = crate::act::act(resumed, "count twice").expect("resumes to completion");
        assert_eq!(answer, "done");
        let state = run::take_run(resumed).expect("end the resumed run");
        assert_eq!(state.status, "completed");
        assert_eq!(state.steps, 3, "three model turns, two replayed");
        assert_eq!(state.tool_calls, 2, "two tool calls, both replayed");
        assert_eq!(
            server.calls(),
            calls_after_crash + 1,
            "only the missing turn reached the provider"
        );
        // The tool bodies ran exactly once each, in the recorded order.
        assert_eq!(
            tool_log()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            first_tool_order.as_slice()
        );
        // The resumed journal has exactly one more record: the missing step.
        let report = state.report_json();
        let journal = state.journal.expect("journal");
        assert!(journal.replaying(), "the resumed run is in replay mode");
        assert_eq!(journal.len(), 5);
        let kinds: Vec<&str> = journal
            .ordered()
            .map(|record| record.kind.as_str())
            .collect();
        assert_eq!(kinds, ["model", "tool", "model", "tool", "model"]);
        // The replayed steps returned the recorded outputs verbatim.
        assert_eq!(
            journal.get(1).expect("step 1").output,
            "2",
            "the first tool result is the recorded one"
        );
        assert_eq!(journal.get(3).expect("step 3").output, "4");
        assert!(!journal.get(4).expect("step 4").output.is_empty());
        assert!(report.contains("\"replay\":true"), "{report}");
        assert!(report.contains("\"status\":\"completed\""), "{report}");

        clear_http_transport();
        crate::tools::clear();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_divergent_step_key_fails_closed_instead_of_replaying() {
        let _guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        spectra_runtime::ffi::clear_host_functions();
        spectra_runtime::register();
        crate::register();
        clear_http_transport();
        crate::tools::clear();

        let dir = std::env::temp_dir().join(format!(
            "spectra-replay-{}",
            crate::journal::new_run_id()
        ));
        let dir_text = dir.to_string_lossy().to_string();
        let handle = start(&dir_text, "divergent");
        let first = resolve(handle, Kind::Assertion, "input-a").expect("first step");
        let Resolved::Fresh(token) = first else {
            panic!("the first step must be fresh");
        };
        commit(handle, &token, None, "true", StepUsage::default(), 0, None).expect("commit");
        run::take_run(handle).expect("end");

        // A resumed run that does something else at the same step is refused:
        // replaying it would either duplicate or silently skip an effect.
        let resumed = start(&dir_text, "divergent");
        let error = resolve(resumed, Kind::Assertion, "input-b").expect_err("divergence");
        assert_eq!(error.kind(), "journal_error");
        assert!(error.detail().contains("divergence"), "{error}");
        run::take_run(resumed).expect("end");

        clear_http_transport();
        std::fs::remove_dir_all(&dir).ok();
    }
}
