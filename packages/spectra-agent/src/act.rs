//! The `act` tool loop (R-3222 T4) and the governed tool dispatch primitive.
//!
//! `act(run, prompt)` runs model turns against the run's provider, exposing the
//! project's registered `#[agent_tool]` functions as model-callable tools. When
//! the model answers without requesting a tool, that answer is the result.
//! Otherwise every requested tool is invoked through [`crate::tools::invoke`]
//! — the single governed dispatch path — and its result is appended to the
//! transcript before the next turn.
//!
//! Ceilings and failures:
//! * `charge_tool_call` runs before each invocation, so a run with
//!   `max_tool_calls` denies the (max+1)-th call without executing it;
//! * the run's budget ceilings are re-checked after every turn;
//! * an unknown tool name or a wrapper failure becomes a tool result carrying
//!   the typed error, so the model can repair its call instead of the run
//!   crashing;
//! * a model that never stops is bounded by [`MAX_ACT_STEPS`].

use crate::error::AgentError;
use crate::hosts::{model_turn_with, snapshot};
use crate::provider::Message;
use crate::run;
use crate::tools;
use crate::trace;

/// Upper bound on model turns in one `act` call. A declared
/// `max_tool_calls`/token/time ceiling is the intended governor; this bound
/// only guarantees termination for an unbounded spec with a non-terminating
/// model.
pub(crate) const MAX_ACT_STEPS: u64 = 64;

/// Runs the `act` loop for `prompt` on `run_handle`.
pub(crate) fn act(run_handle: i64, prompt: &str) -> Result<String, AgentError> {
    // I7: every tool reachable from this run must be inside the run's grant
    // before the first dispatch.
    tools::enforce_run_grant(run_handle)?;
    let spec = snapshot(run_handle)?.0;
    // R-3217 T5: the loop's planning turn is the `plan` span.
    let run_id = run::with_run(run_handle, |state| state.run_id.clone())?;
    trace::emit_plan(&run_id, &spec.goal, Some(prompt));

    let definitions = tools::definitions_for(run_handle)?;
    let mut messages = vec![Message::user(prompt)];
    for _ in 0..MAX_ACT_STEPS {
        let response = model_turn_with(run_handle, messages.clone(), None, definitions.clone())?;
        if response.tool_calls.is_empty() {
            return Ok(response.text);
        }
        if !response.text.is_empty() {
            messages.push(Message::assistant(response.text.clone()));
        }

        for call in &response.tool_calls {
            match tools::invoke(run_handle, &call.name, &call.arguments) {
                Ok(result) => messages.push(Message::tool(&call.name, result)),
                // An unknown tool or a wrapper failure is exactly what the next
                // turn is for: it reaches the model as a tool result, so a run
                // degrades gracefully instead of crashing.
                Err(error @ (AgentError::UnknownTool(_) | AgentError::ToolFailed(_))) => {
                    messages.push(Message::tool(
                        &call.name,
                        format!("error: {}", error.message()),
                    ));
                }
                // Anything else (a crossed budget ceiling, a capability denial)
                // is terminal by contract: the run is already cancelled and
                // every later call would be refused.
                Err(error) => return Err(error),
            }
        }

        // A crossed wall-clock/token/cost ceiling cancels the run and the next
        // accessor refuses; re-checking here makes the cancellation visible at
        // the loop boundary rather than at the next model call.
        run::with_run(run_handle, |state| {
            crate::budget::guard(state, None)
        })??;
    }
    Err(AgentError::ToolLoopCeiling(format!(
        "act exceeded {MAX_ACT_STEPS} model turns without a final answer"
    )))
}

/// `tool_call(run, name, args_json)`: one governed tool invocation.
///
/// The dispatcher primitive `act` is built on. Exposed so the tool ABI is
/// independently testable in JIT and AOT (R-3222-T1 spike evidence) and so
/// R-3218/R-3219 reuse exactly this path rather than adding a second one.
pub(crate) fn tool_call(
    run_handle: i64,
    name: &str,
    arguments: &str,
) -> Result<String, AgentError> {
    tools::enforce_run_grant(run_handle)?;
    tools::invoke(run_handle, name, arguments)
}
