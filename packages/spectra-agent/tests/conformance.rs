//! R-3221 T5 — crate-level conformance suite for the Phase 32 agent platform.
//!
//! This integration test aggregates the phase's cross-cutting invariants at the
//! only boundary a consumer of `spectra-agent` actually observes: the registered
//! host functions (`spectra.std.agent.*`), the runtime's single generic dispatch
//! seam (`spectra_rt_host_invoke`), and the crate's public seams (the approver
//! and the injected HTTP transport). Nothing here reaches into crate internals,
//! so a green suite is evidence about the shipped ABI, not about private
//! helpers:
//!
//! 1. capability denial through the governed dispatch (`tool_call` and `act`);
//! 2. journal replay without duplicated effects (the tool wrapper runs once);
//! 3. budget cancellation at the token ceiling;
//! 4. approval default deny without an attached approver;
//! 5. the taint policy matrix (block / approve / allow) at the dispatch seam;
//! 6. compensation executed exactly once per rollback, and not re-executed on
//!    replay;
//! 7. an MCP round trip over the injected transport, through the governed
//!    dispatch;
//! 8. surface determinism (`spectralang surface --json`), shelled out to the
//!    CLI when the binary exists and skipped with an explicit message when it
//!    does not — the conformance validator covers it either way.
//!
//! The crate's global state (host registry, tool registry, approver, HTTP
//! transport, run table, policy evaluator) is process-wide by design, so every
//! case here runs under one mutex. The tool registry is the one piece an
//! integration test cannot clear (there is no public reset): once the MCP case
//! registers a discovered remote entry, every later run in this process sees
//! it in the registered set, which `tools::enforce_run_grant` checks before a
//! dispatch. The suite's specs therefore grant the union of the effects its
//! tools declare (`["spectra.std.fs","mcp"]`); the denial case deliberately
//! grants nothing, which is the invariant it proves.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use spectra_agent::{
    clear_http_transport, set_approver, set_http_transport, ApprovalRequest, Approver, Decision,
    HttpTransport, TransportResponse,
};
use spectra_runtime::agent::run_context;
use spectra_runtime::ffi::{
    clear_host_functions, lookup_host_function, spectra_rt_host_invoke, SpectraHostCallContext,
    SpectraHostValue, HOST_STATUS_DENIED, HOST_STATUS_SUCCESS,
};
use spectra_runtime::tracing::alloc_string;

/// Serializes every case: the state they exercise is process-global.
static GLOBAL: Mutex<()> = Mutex::new(());

/// Invocations of the counting tool wrapper (the effect whose duplication the
/// replay case forbids).
static EFFECTS: AtomicUsize = AtomicUsize::new(0);

/// Set if the wrapper of the ungranted tool is ever reached.
static DENIED_TOOL_REACHED: AtomicBool = AtomicBool::new(false);

// ── the ABI helpers ──────────────────────────────────────────────────────

/// Allocates a Spectra string argument in the runtime arena.
fn s(value: &str) -> i64 {
    unsafe { alloc_string(value) }
}

/// Reads a NUL-terminated packed UTF-8 string.
fn text(pointer: i64) -> String {
    if pointer == 0 {
        return String::new();
    }
    let ptr = pointer as *const u8;
    let mut bytes = Vec::new();
    for offset in 0..(16 * 1024 * 1024usize) {
        let byte = unsafe { *ptr.add(offset) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    String::from_utf8(bytes).unwrap_or_default()
}

/// Calls a registered host function directly (the test's stand-in for one
/// compiled host call).
fn call(name: &str, args: &[SpectraHostValue]) -> (i32, SpectraHostValue) {
    let function = lookup_host_function(name).unwrap_or_else(|| panic!("{name} is not registered"));
    let mut results = [0 as SpectraHostValue; 1];
    let mut context = SpectraHostCallContext {
        args: args.as_ptr(),
        arg_len: args.len(),
        results: results.as_mut_ptr(),
        result_len: results.len(),
        invoke_fn: None,
    };
    (function(&mut context), results[0])
}

/// Dispatches a host call through the runtime's generic entrypoint, which is
/// where the capability and taint evaluators are enforced.
fn dispatch(name: &str, args: &[SpectraHostValue]) -> i32 {
    let mut results = [0 as SpectraHostValue; 1];
    spectra_rt_host_invoke(
        name.as_ptr(),
        name.len(),
        args.as_ptr(),
        args.len(),
        results.as_mut_ptr(),
        results.len(),
    )
}

/// The tag of a two-word `Result` (`0` = Ok, `1` = Err).
fn tag(value: i64) -> i64 {
    unsafe { *(value as *const i64) }
}

/// The payload word of a `Result`.
fn payload(value: i64) -> i64 {
    unsafe { *(value as *const i64).add(1) }
}

/// The `Ok` payload, panicking (with the typed message) on an `Err`.
fn ok(value: i64) -> i64 {
    if tag(value) != 0 {
        panic!("expected Ok, got Err: {}", error_message(value));
    }
    payload(value)
}

/// The message of an `Err` result, read through `std.error.message`.
fn error_message(value: i64) -> String {
    assert_eq!(tag(value), 1, "expected an Err result");
    let (status, message) = call("spectra.std.error.message", &[payload(value)]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    text(message)
}

/// Runs an async host to completion (the tests' `block_on`).
fn awaited(name: &str, args: &[SpectraHostValue]) -> i64 {
    let (status, task) = call(name, args);
    assert_eq!(status, HOST_STATUS_SUCCESS, "{name} did not return a task");
    spectra_runtime::stdlib::block_on_task_value(task).expect("the task resolves")
}

/// Starts a run and returns its handle.
fn start(spec: &str) -> i64 {
    let (status, outcome) = call("spectra.std.agent.agent_start", &[s(spec)]);
    assert_eq!(status, HOST_STATUS_SUCCESS, "agent_start status");
    ok(outcome)
}

/// Ends a run and returns its report document.
fn end(run: i64) -> String {
    let (status, outcome) = call("spectra.std.agent.agent_end", &[run]);
    assert_eq!(status, HOST_STATUS_SUCCESS, "agent_end status");
    text(ok(outcome))
}

fn remaining(run: i64) -> i64 {
    let (status, outcome) = call("spectra.std.agent.budget_remaining", &[run]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    ok(outcome)
}

/// A temporary work directory under `target/`, cleaned before use.
///
/// The directory is namespaced by process id: the certification gate runs
/// `cargo test -p spectra-agent` from several validators at once during its
/// parallel sweep, and each test process owns its paths.
fn work_dir(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/r3221-agent-conformance/conformance")
        .join(format!("p{}", std::process::id()));
    let dir = root.join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work directory");
    std::fs::canonicalize(&dir).expect("canonical work directory")
}

/// A journal directory as a JSON-safe string (forward slashes, no verbatim
/// Windows prefix).
fn journal_dir(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("//?/").map(str::to_string).unwrap_or(text)
}

// ── the tool wrappers (ADR 0019 ABI) ─────────────────────────────────────

/// Writes the JSON result of one tool call into the out-slot.
fn write_result(out_slot: i64, document: &str) -> i64 {
    let pointer = unsafe { alloc_string(document) };
    if pointer == 0 {
        return 1;
    }
    unsafe { *(out_slot as *mut i64) = pointer };
    0
}

/// Counts its invocations and answers with the new count: the external effect
/// the replay case must not repeat.
extern "C" fn count_wrapper(_run: i64, _arguments: i64, out_slot: i64) -> i64 {
    let count = EFFECTS.fetch_add(1, Ordering::SeqCst) + 1;
    write_result(out_slot, &format!("{{\"count\":{count}}}"))
}

/// The wrapper of the tool whose derived effect the denial case withholds.
extern "C" fn denied_wrapper(_run: i64, _arguments: i64, _out_slot: i64) -> i64 {
    DENIED_TOOL_REACHED.store(true, Ordering::SeqCst);
    1
}

/// Registers the suite's tools (idempotent by name and address).
fn register_tools() {
    for (name, address, effects) in [
        (
            "conformance_count",
            count_wrapper as *const () as usize as i64,
            "[]",
        ),
        (
            "conformance_denied",
            denied_wrapper as *const () as usize as i64,
            r#"["spectra.std.fs.fs_write"]"#,
        ),
    ] {
        let (status, outcome) = call(
            "spectra.std.agent.register_tool",
            &[
                s(name),
                address,
                s("conformance suite tool"),
                s(r#"{"type":"object"}"#),
                s(effects),
            ],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS, "register_tool({name}) status");
        assert_eq!(tag(outcome), 0, "register_tool({name}) failed");
    }
}

/// Puts the process-global state into a known state and returns the lock guard.
fn setup() -> MutexGuard<'static, ()> {
    let guard = GLOBAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    clear_host_functions();
    spectra_runtime::register();
    spectra_agent::register();
    clear_http_transport();
    set_approver(None);
    register_tools();
    guard
}

// ── 1. capability denial through the governed dispatch ───────────────────

#[test]
fn capability_denial_refuses_tool_call_and_act_before_the_effect() {
    let _guard = setup();
    DENIED_TOOL_REACHED.store(false, Ordering::SeqCst);

    // A run with no grant at all: the registered tool's derived effect
    // (`spectra.std.fs.fs_write`) is outside its authority.
    let run = start(
        r#"{"goal":"conformance-denial","model":"mock/echo","endpoint":"mock:","allow":[],"journal":""}"#,
    );

    let direct = awaited(
        "spectra.std.agent.tool_call",
        &[run, s("conformance_denied"), s("{}")],
    );
    assert_eq!(tag(direct), 1, "tool_call must be refused");
    let message = error_message(direct);
    assert!(message.contains("capability_denied"), "{message}");
    assert!(message.contains("conformance_denied"), "{message}");
    assert!(
        !DENIED_TOOL_REACHED.load(Ordering::SeqCst),
        "the wrapper must never run when the grant does not cover its effect"
    );

    // The same check guards `act`, before its first dispatch.
    let looped = awaited(
        "spectra.std.agent.act",
        &[run, s("spectra:final=never reached")],
    );
    assert_eq!(tag(looped), 1, "act must be refused");
    assert!(
        error_message(looped).contains("capability_denied"),
        "{}",
        error_message(looped)
    );
    assert!(!DENIED_TOOL_REACHED.load(Ordering::SeqCst));

    // A run that grants the effect dispatches the same tool without denial.
    let granted = start(
        r#"{"goal":"conformance-denial-granted","model":"mock/echo","endpoint":"mock:","allow":["spectra.std.fs","mcp"],"journal":""}"#,
    );
    let allowed = awaited(
        "spectra.std.agent.tool_call",
        &[granted, s("conformance_denied"), s("{}")],
    );
    assert_eq!(
        tag(allowed),
        1,
        "the tool itself fails, but the dispatch is authorized"
    );
    assert!(
        !error_message(allowed).contains("capability_denied"),
        "{}",
        error_message(allowed)
    );

    let _ = end(run);
    let _ = end(granted);
}

// ── 2. journal replay without duplicated effects ─────────────────────────

#[test]
fn journal_replay_returns_the_recorded_result_without_repeating_the_effect() {
    let _guard = setup();
    let dir = work_dir("replay");
    let journal = journal_dir(&dir);
    let spec = format!(
        r#"{{"goal":"conformance-replay","model":"mock/echo","endpoint":"mock:","allow":["spectra.std.fs","mcp"],"max_tool_calls":0,"untrusted":"allow","journal":"{journal}","journal_payloads":true,"run_id":"conformance-replay"}}"#
    );
    let script = "spectra:tool=conformance_count {}\nspectra:final=replay complete";

    // Fresh run: the wrapper performs the effect exactly once.
    EFFECTS.store(0, Ordering::SeqCst);
    let run = start(&spec);
    let answer = awaited("spectra.std.agent.act", &[run, s(script)]);
    assert_eq!(text(ok(answer)), "replay complete");
    assert_eq!(EFFECTS.load(Ordering::SeqCst), 1, "one effect on the fresh run");
    let report = end(run);
    assert!(report.contains("\"replay\":false"), "{report}");
    assert!(report.contains("\"tool_calls\":1"), "{report}");

    // The run is durable: the tool step is on disk.
    let journal_file = dir.join("conformance-replay.jsonl");
    let recorded = std::fs::read_to_string(&journal_file).expect("journal file");
    assert!(recorded.contains("\"kind\":\"tool\""), "{recorded}");

    // Replay: same run id, same journal, same script. The recorded tool result
    // is returned and the effect is not performed a second time.
    let run = start(&spec);
    let answer = awaited("spectra.std.agent.act", &[run, s(script)]);
    assert_eq!(text(ok(answer)), "replay complete");
    assert_eq!(
        EFFECTS.load(Ordering::SeqCst),
        1,
        "replay must return the recorded result instead of re-executing the effect"
    );
    let report = end(run);
    assert!(report.contains("\"replay\":true"), "{report}");
    assert!(report.contains("\"tool_calls\":1"), "{report}");
}

// ── 3. budget cancellation ───────────────────────────────────────────────

#[test]
fn a_token_ceiling_is_accounted_then_refuses_every_later_call() {
    let _guard = setup();
    // The mock reports 1 input + 3 output tokens for each echoed turn, so two
    // turns take the run from 0 to 8 of 6 and the run is cancelled.
    let run = start(
        r#"{"goal":"conformance-budget","model":"mock/echo","endpoint":"mock:","allow":[],"max_tokens":6,"journal":""}"#,
    );
    assert_eq!(remaining(run), 6);

    let first = awaited("spectra.std.agent.ask", &[run, s("alpha")]);
    assert_eq!(tag(first), 0, "{}", error_message(first));
    assert_eq!(remaining(run), 2);

    // The crossing turn is accounted and still delivers its value.
    let second = awaited("spectra.std.agent.ask", &[run, s("beta")]);
    assert_eq!(tag(second), 0);
    assert_eq!(remaining(run), 0);

    // Every later call is refused before it reaches the provider.
    let third = awaited("spectra.std.agent.ask", &[run, s("gamma")]);
    assert_eq!(tag(third), 1, "the run must be cancelled");
    assert!(error_message(third).contains("budget_exceeded"), "{}", error_message(third));

    let report = end(run);
    assert!(report.contains("\"status\":\"budget_exceeded\""), "{report}");
    assert!(report.contains("\"ceiling\":\"max_tokens\""), "{report}");
}

// ── 4. approval default deny ─────────────────────────────────────────────

#[test]
fn approval_denies_without_an_approver_and_allows_with_one() {
    let _guard = setup();
    let run = start(
        r#"{"goal":"conformance-approval","model":"mock/echo","endpoint":"mock:","allow":[],"journal":""}"#,
    );

    // No approver is attached: the decision is a deny, and the call succeeds
    // with `false` rather than failing.
    let (status, outcome) = call("spectra.std.agent.approve", &[run, s("spectra.std.fs.fs_write")]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert_eq!(ok(outcome), 0, "absent an approver the decision must be deny");

    // An attached approver is asked, and its answer is honored. The approver
    // runs inside a host call, so it must not panic: it flips a flag instead.
    static APPROVER_ASKED: AtomicBool = AtomicBool::new(false);
    struct AllowOnce;
    impl Approver for AllowOnce {
        fn decide(&self, _request: &ApprovalRequest) -> Decision {
            APPROVER_ASKED.store(true, Ordering::SeqCst);
            Decision::AllowOnce {
                by: "conformance suite".to_string(),
            }
        }
    }
    assert!(set_approver(Some(Arc::new(AllowOnce))));
    let (status, outcome) = call(
        "spectra.std.agent.approve",
        &[run, s("spectra.std.agent.ask")],
    );
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert_eq!(ok(outcome), 1, "an attached approver's allow-once is honored");
    assert!(
        APPROVER_ASKED.load(Ordering::SeqCst),
        "the attached approver must be the one asked"
    );
    set_approver(None);

    let report = end(run);
    assert!(report.contains("\"status\":\"completed\""), "{report}");
}

// ── 5. the taint policy matrix ───────────────────────────────────────────

/// One row of the taint matrix: a run holding untrusted content, with the
/// given policy and (optional) approver, must gate the catalog sink at the
/// dispatch seam exactly as `expect_denied` says.
fn taint_row(policy: &str, approver: Option<Arc<dyn Approver>>, expect_denied: bool) {
    let spec = format!(
        r#"{{"goal":"conformance-taint-{policy}","model":"mock/echo","endpoint":"mock:","allow":["spectra.std.fs"],"untrusted":"{policy}","journal":""}}"#
    );
    let run = start(&spec);

    // The value enters from an external origin and is recorded untrusted. The
    // host returns it unchanged, so the ledger is the only effect.
    let (status, outcome) = call(
        "spectra.std.agent.untrusted",
        &[run, s("poisoned payload"), s("external:conformance")],
    );
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert_eq!(text(ok(outcome)), "poisoned payload");

    set_approver(approver);
    // The run is active on this thread's chain, exactly as it is while a
    // run-scoped host executes.
    let chain = run_context::push(run as u64);
    let denied = dispatch("spectra.std.fs.fs_write", &[]) == HOST_STATUS_DENIED;
    drop(chain);

    assert_eq!(
        denied, expect_denied,
        "policy '{policy}' must {} the sink it is not allowed to reach",
        if expect_denied { "refuse" } else { "allow" }
    );
    set_approver(None);

    // The capability decision is independent of taint: the run granted the
    // namespace, so the call reaches the host when the gate allows it.
    let spec_ungranted = format!(
        r#"{{"goal":"conformance-taint-{policy}-ungranted","model":"mock/echo","endpoint":"mock:","allow":[],"untrusted":"{policy}","journal":""}}"#
    );
    let ungranted = start(&spec_ungranted);
    let (status, outcome) = call(
        "spectra.std.agent.untrusted",
        &[ungranted, s("poisoned payload"), s("external:conformance")],
    );
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert_eq!(text(ok(outcome)), "poisoned payload");
    let chain = run_context::push(ungranted as u64);
    assert_eq!(
        dispatch("spectra.std.fs.fs_write", &[]),
        HOST_STATUS_DENIED,
        "an ungranted sink is denied whatever the taint policy"
    );
    drop(chain);

    let _ = end(run);
    let _ = end(ungranted);
}

#[test]
fn taint_policy_matrix_gates_sinks_at_the_dispatch_seam() {
    let _guard = setup();

    struct AllowTaint;
    impl Approver for AllowTaint {
        fn decide(&self, _request: &ApprovalRequest) -> Decision {
            Decision::AllowOnce {
                by: "conformance suite".to_string(),
            }
        }
    }

    // block: denied outright.
    taint_row("block", None, true);
    // approve without an attached approver: default deny.
    taint_row("approve", None, true);
    // approve with an approver that allows: the gate opens.
    taint_row("approve", Some(Arc::new(AllowTaint)), false);
    // allow: recorded and permitted.
    taint_row("allow", None, false);
}

// ── 6. compensation exactly once ─────────────────────────────────────────

#[test]
fn compensation_executes_exactly_once_per_rollback() {
    let _guard = setup();
    let dir = work_dir("compensation");
    let journal = journal_dir(&dir);
    let spec = format!(
        r#"{{"goal":"conformance-compensation","model":"mock/echo","endpoint":"mock:","allow":["spectra.std.fs","mcp"],"untrusted":"allow","journal":"{journal}","journal_payloads":true,"run_id":"conformance-compensation"}}"#
    );

    EFFECTS.store(0, Ordering::SeqCst);
    let run = start(&spec);
    for arguments in [r#"{"n":1}"#, r#"{"n":2}"#] {
        let (status, outcome) = call(
            "spectra.std.agent.compensate",
            &[run, s("conformance_count"), s(arguments)],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(ok(outcome), 1, "the declaration is accepted");
    }

    // LIFO: both pending compensations execute once each.
    let (status, outcome) = call("spectra.std.agent.rollback", &[run, s("conformance disaster")]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert_eq!(ok(outcome), 2, "two declared compensations execute");
    assert_eq!(EFFECTS.load(Ordering::SeqCst), 2);

    // A second rollback has nothing pending: nothing executes again.
    let (status, outcome) = call("spectra.std.agent.rollback", &[run, s("conformance disaster")]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert_eq!(ok(outcome), 0);
    assert_eq!(EFFECTS.load(Ordering::SeqCst), 2, "a compensation runs once");

    let report = end(run);
    assert!(report.contains("\"status\":\"rolled_back\""), "{report}");
    assert!(report.contains("\"compensations_pending\":0"), "{report}");

    // The attempts are durable: two declarations and two executions.
    let recorded = std::fs::read_to_string(dir.join("conformance-compensation.jsonl"))
        .expect("journal file");
    assert_eq!(recorded.matches("\"kind\":\"compensation\"").count(), 2, "{recorded}");
    assert_eq!(recorded.matches("\"kind\":\"rollback\"").count(), 2, "{recorded}");

    // Replay: the same declarations rebuild the pending stack, and the same
    // rollback returns the recorded outcomes without re-executing the tools.
    let run = start(&spec);
    for arguments in [r#"{"n":1}"#, r#"{"n":2}"#] {
        let (status, outcome) = call(
            "spectra.std.agent.compensate",
            &[run, s("conformance_count"), s(arguments)],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(ok(outcome), 1);
    }
    let (status, outcome) = call("spectra.std.agent.rollback", &[run, s("conformance disaster")]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert_eq!(ok(outcome), 2, "the replayed rollback reports its recorded walk");
    assert_eq!(
        EFFECTS.load(Ordering::SeqCst),
        2,
        "a replayed compensation must not execute the effect again"
    );
    let _ = end(run);
}

// ── 7. MCP round trip over the injected transport ────────────────────────

/// A stub MCP server: the crate's injected transport is the seam the embedding
/// application owns, so the round trip needs no socket and no network.
struct StubServer;

impl HttpTransport for StubServer {
    fn post_json(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        body: &str,
    ) -> Result<TransportResponse, String> {
        if body.contains("notifications/initialized") {
            return Ok(TransportResponse {
                status: 202,
                body: String::new(),
            });
        }
        let reply = if body.contains("\"tools/list\"") {
            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"echoes text","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}]}}"#
        } else if body.contains("\"tools/call\"") {
            r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"remote echo: hi"}],"isError":false}}"#
        } else {
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"stub","version":"1.0.0"}}}"#
        };
        Ok(TransportResponse {
            status: 200,
            body: reply.to_string(),
        })
    }
}

#[test]
fn mcp_discovery_and_invocation_flow_through_the_governed_dispatch() {
    let _guard = setup();
    assert!(set_http_transport(StubServer), "the transport is installed");

    let run = start(
        r#"{"goal":"conformance-mcp","model":"mock/echo","endpoint":"mock:","allow":["spectra.std.fs","mcp.evil.test"],"untrusted":"block","journal":""}"#,
    );

    let document = awaited(
        "spectra.std.agent.mcp_connect",
        &[run, s("http://evil.test/mcp")],
    );
    let document = text(ok(document));
    assert!(document.contains("\"server\":\"mcp.evil.test\""), "{document}");
    assert!(document.contains("\"remoteName\":\"echo\""), "{document}");

    // The discovered tool is an ordinary registry entry, invoked through the
    // same governed dispatch as a compiled tool.
    let invoked = awaited(
        "spectra.std.agent.tool_call",
        &[run, s("mcp__evil_test__echo"), s(r#"{"text":"hi"}"#)],
    );
    assert_eq!(text(ok(invoked)), "remote echo: hi");

    // Discovery and the invocation are governed steps: the call is charged.
    let report = end(run);
    assert!(report.contains("\"status\":\"completed\""), "{report}");
    assert!(report.contains("\"tool_calls\":1"), "{report}");

    clear_http_transport();
}

// ── 8. surface determinism ───────────────────────────────────────────────

/// The CLI binary, when it has been built.
fn cli_binary() -> Option<PathBuf> {
    if let Some(override_path) = std::env::var_os("SPECTRALANG_BINARY") {
        let path = PathBuf::from(override_path);
        return path.is_file().then_some(path);
    }
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug");
    ["spectralang.exe", "spectralang"]
        .iter()
        .map(|name| target.join(name))
        .find(|path| path.is_file())
}

fn surface_json(cli: &Path, project: &Path) -> String {
    let output = Command::new(cli)
        .args(["surface", "--json"])
        .arg(project)
        .output()
        .expect("surface runs");
    assert!(output.status.success(), "surface exited with {}", output.status);
    String::from_utf8(output.stdout).expect("surface emits UTF-8 JSON")
}

#[test]
fn surface_json_is_byte_identical_across_runs() {
    let Some(cli) = cli_binary() else {
        eprintln!(
            "conformance: surface determinism skipped here — no spectralang binary under \
             target/debug; scripts/validate_r3221_agent_conformance.py runs this check with a \
             built CLI"
        );
        return;
    };
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/projects/valid/integrated_agent_service");
    let first = surface_json(&cli, &project);
    let second = surface_json(&cli, &project);
    assert!(!first.trim().is_empty(), "surface output is empty");
    assert_eq!(
        first, second,
        "two surface --json runs over the same project must be byte-identical"
    );
}
