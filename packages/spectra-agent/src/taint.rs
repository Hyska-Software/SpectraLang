//! Message and handle taint: the provenance ledger, the catalog-derived sink
//! set, and the gate at the single dispatch seam (R-3223 T1–T3).
//!
//! # Honest granularity
//!
//! The host ABI carries `i64` scalars and handles, so there is no string-level
//! information flow to track. What this module does track, and what it does
//! not, is frozen here:
//!
//! * provenance is per **content digest** and per message. The ledger records
//!   that a value with this digest entered the run, from which origin, and
//!   whether it has been declassified — it records nothing about how the value
//!   was transformed afterwards. A value derived from an untrusted one has a
//!   different digest and stays untrusted, so `trust` can never launder a
//!   transformation the ledger did not see.
//! * a **run** is tainted when any of its ledger entries is untrusted. A sink
//!   is gated on "this run chain holds untrusted content", not on the dataflow
//!   between a particular value and a particular argument. This is the
//!   conservative direction: it can require approval for a sink that happens
//!   not to touch the untrusted value, and it can never let one through
//!   silently.
//! * taint is **monotone**: an untrusted entry is never downgraded by
//!   observation. Only `trust(run, value, reason)` declassifies, and only the
//!   exact digest it names.
//!
//! # Sinks
//!
//! The sink set is derived from the contract catalog (`sink = true`, i.e. the
//! entry's effects contain `mutation`) — never from a hand-written list, so a
//! namespace cannot drift out of the classification. A sink's declared
//! `scope_keys` are read from the *dispatch arguments* while they are live; a
//! key without an extractor beside the host call is gated at name level (the
//! call is a sink for every argument value). See [`scope_value`].
//!
//! # The gate
//!
//! Consulted after the capability check, so a call outside the run's grant is
//! refused as a capability denial whatever the taint state. With untrusted
//! content in the chain and a sink target, the run's `AgentSpec.untrusted`
//! policy decides:
//!
//! * `block` — deny with a `trust_required` reason (the caller can declassify
//!   with `trust` or ask for a run whose policy permits it);
//! * `approve` (the default) — route through the approval registry, which
//!   fails closed when no approver is attached;
//! * `allow` — proceed, journaling the decision so the choice is auditable.
//!
//! Every decision and every provenance/declassification entry is journaled (a
//! `taint` record), so a replayed run does not ask twice and the decision is
//! durable.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use spectra_runtime::agent::policy_hook::PolicyDecision;
use spectra_runtime::ffi::SpectraHostValue;

use crate::abi;
use crate::approval;
use crate::digest;
use crate::error::AgentError;
use crate::journal::StepUsage;
use crate::provider::Message;
use crate::replay::{self, Kind, Resolved};
use crate::run;
use crate::spec::UntrustedPolicy;

/// Origin tag of a prompt or a value the program itself supplied.
pub(crate) const ORIGIN_USER: &str = "user";

/// Origin tag of model output.
pub(crate) const ORIGIN_MODEL: &str = "model";

/// Origin tag of a tool result.
pub(crate) fn origin_tool(name: &str) -> String {
    format!("tool:{name}")
}

/// Upper bound on an origin/reason tag. The tag is journal attribution, so a
/// pathological argument cannot make the journal unbounded.
pub(crate) const MAX_TAG_LEN: usize = 256;

/// One ledger entry: the provenance of one content digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The content digest this entry is keyed by (reused as the journal
    /// record's output, so the ledger key never has to be recomputed).
    pub digest: String,
    /// `user` | `model` | `tool:<name>` | `external:<source>`.
    pub origin: String,
    /// Whether the digest has been declassified.
    pub trusted: bool,
    /// The declassification reason; empty while the entry is untrusted.
    pub reason: String,
}

/// The per-run provenance ledger.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    entries: BTreeMap<String, Entry>,
}

impl Ledger {
    /// Records untrusted provenance. Untrusted always wins: a later
    /// observation of the same digest cannot launder it.
    pub(crate) fn mark_untrusted(&mut self, value: &str, origin: &str) -> Entry {
        let entry = Entry {
            digest: digest::of(value),
            origin: origin.to_string(),
            trusted: false,
            reason: String::new(),
        };
        self.entries.insert(entry.digest.clone(), entry.clone());
        entry
    }

    /// Records a trusted observation without declassifying: an existing
    /// untrusted entry keeps its mark (taint is monotone).
    pub(crate) fn observe_trusted(&mut self, value: &str, origin: &str) -> Entry {
        let digest = digest::of(value);
        if let Some(existing) = self.entries.get(&digest) {
            return existing.clone();
        }
        let entry = Entry {
            digest,
            origin: origin.to_string(),
            trusted: true,
            reason: String::new(),
        };
        self.entries.insert(entry.digest.clone(), entry.clone());
        entry
    }

    /// Declassifies one digest. The reason is mandatory and becomes the audit
    /// record; the entry keeps the origin it was first seen with.
    pub(crate) fn declassify(&mut self, value: &str, reason: &str) -> Entry {
        let digest = digest::of(value);
        let origin = self
            .entries
            .get(&digest)
            .map(|entry| entry.origin.clone())
            .unwrap_or_else(|| ORIGIN_USER.to_string());
        let entry = Entry {
            digest,
            origin,
            trusted: true,
            reason: reason.to_string(),
        };
        self.entries.insert(entry.digest.clone(), entry.clone());
        entry
    }

    /// Whether any entry is still untrusted.
    pub(crate) fn holds_untrusted(&self) -> bool {
        self.entries.values().any(|entry| !entry.trusted)
    }

    /// The origins of the untrusted entries, deduplicated, in digest order.
    pub(crate) fn untrusted_origins(&self) -> Vec<String> {
        let mut origins: Vec<String> = self
            .entries
            .values()
            .filter(|entry| !entry.trusted)
            .map(|entry| entry.origin.clone())
            .collect();
        origins.sort_unstable();
        origins.dedup();
        origins
    }

    /// Test accessor: number of ledger entries.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

// ── sink classification ──────────────────────────────────────────────────

/// One catalog-classified sensitive sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sink {
    /// Catalog path (`std.fs.fs_write`), for diagnostics.
    pub path: String,
    /// Scope keys the catalog declares for this host call.
    pub scope_keys: Vec<String>,
}

/// The sink set, derived once from the embedded contract catalog.
///
/// The catalog is the single source of truth (`sink = true` ⇔ the entry's
/// effects contain `mutation`); this map only keys it by the runtime host-call
/// name the dispatch seam evaluates.
fn sinks() -> &'static HashMap<String, Sink> {
    static SINKS: LazyLock<HashMap<String, Sink>> = LazyLock::new(|| {
        spectra_contract::catalog()
            .entry
            .into_iter()
            .filter(|entry| entry.sink)
            .map(|entry| {
                (
                    entry.binding,
                    Sink {
                        path: entry.path,
                        scope_keys: entry.scope_keys,
                    },
                )
            })
            .collect()
    });
    &SINKS
}

/// The sink classification for a runtime host-call name, if it is a sink.
pub(crate) fn sink(host_call: &str) -> Option<&'static Sink> {
    sinks().get(host_call)
}

/// Whether the runtime host-call name is a catalog-classified sink.
pub(crate) fn is_sink(host_call: &str) -> bool {
    sinks().contains_key(host_call)
}

/// Reads one declared scope key from a sink's dispatch arguments.
///
/// This is the scope-extractor registry. An extractor exists only where the
/// value is a direct argument of the host call; a key with no extractor is
/// evaluated at name level (the call is a sink for every argument value),
/// which is the conservative direction.
///
/// `spectra.api.client.request` declares `method`, but the method lives inside
/// the `std.api.http.Request` handle owned by `packages/spectra-api`: decoding
/// it here would invert the crate dependency, so the extractor is deliberately
/// absent and the deferral is recorded in `docs/agent-platform.md`.
pub(crate) fn scope_value(
    host_call: &str,
    key: &str,
    args: &[SpectraHostValue],
) -> Option<String> {
    match (host_call, key) {
        // `apply_sqlite(connection, table)`: the migration target is the
        // second argument, a plain string.
        ("spectra.api.db.migrate.apply_sqlite", "table") => {
            args.get(1).and_then(|value| abi::read_string_arg(*value))
        }
        _ => None,
    }
}

/// The scope note recorded with a decision: `", scope: table=…"`, empty when
/// no declared key has an extractor. Never contains a payload beyond the
/// argument the catalog declares as the sink's scope.
fn scope_note(host_call: &str, args: &[SpectraHostValue]) -> String {
    let Some(sink) = sink(host_call) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for key in &sink.scope_keys {
        if let Some(value) = scope_value(host_call, key, args) {
            parts.push(format!("{key}={value}"));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(", scope: {}", parts.join(","))
    }
}

// ── the gate ─────────────────────────────────────────────────────────────

/// The taint decision for one host call.
///
/// `chain` is the active run chain (outermost first) as the dispatch seam saw
/// it; the innermost live run is the caller whose journal records the
/// decision. Returns [`PolicyDecision::Allow`] when the target is not a
/// catalog sink or no run on the chain holds untrusted content — the two cases
/// that keep a program without a run exactly as it was (invariant I5).
pub(crate) fn gate(chain: &[u64], host_call: &str, args: &[SpectraHostValue]) -> PolicyDecision {
    if !is_sink(host_call) {
        return PolicyDecision::Allow;
    }
    let tainted: Vec<run::RunTaint> = chain
        .iter()
        .filter_map(|raw| run::taint_for_raw_id(*raw))
        .filter(|state| state.holds_untrusted)
        .collect();
    if tainted.is_empty() {
        return PolicyDecision::Allow;
    }
    // The caller is the innermost live run on the chain; it owns the journal
    // the decision is recorded in (and the approval registry entry).
    let Some(caller) = chain.iter().rev().find_map(|raw| run::taint_for_raw_id(*raw)) else {
        return PolicyDecision::Allow;
    };
    // A nested run can narrow the policy but never loosen it: the strictest
    // policy on the chain wins (block > approve > allow), exactly like the
    // capability intersection.
    let policy = strictest(tainted.iter().map(|state| state.policy));
    match policy {
        UntrustedPolicy::Allow => {
            let attribution = format!(
                "taint policy 'allow' proceeded with untrusted content from {}{}",
                origins_text(&tainted),
                scope_note(host_call, args)
            );
            let _ = journal_decision(&caller, host_call, "allow", Some(attribution));
            PolicyDecision::Allow
        }
        UntrustedPolicy::Block => {
            let reason = denial_reason(&caller, &tainted, host_call, policy, &scope_note(host_call, args));
            // The denial is durable before the trap consumes it.
            let _ = journal_decision(&caller, host_call, "deny", Some(reason.clone()));
            PolicyDecision::Deny { reason }
        }
        UntrustedPolicy::Approve => {
            let action = format!("trust_required:{host_call}");
            match approval::approve(caller.handle, &action) {
                Ok(true) => PolicyDecision::Allow,
                Ok(false) => {
                    let reason = format!(
                        "{}; the attached approver denied the action",
                        denial_reason(&caller, &tainted, host_call, policy, &scope_note(host_call, args))
                    );
                    let _ = journal_decision(&caller, host_call, "deny", Some(reason.clone()));
                    PolicyDecision::Deny { reason }
                }
                Err(error) => {
                    // A journal or handle failure must fail closed.
                    let reason = format!(
                        "{}; the approval could not be recorded: {}",
                        denial_reason(&caller, &tainted, host_call, policy, &scope_note(host_call, args)),
                        error.message()
                    );
                    PolicyDecision::Deny { reason }
                }
            }
        }
    }
}

/// The strictest of the chain's taint policies.
fn strictest(policies: impl Iterator<Item = UntrustedPolicy>) -> UntrustedPolicy {
    policies.fold(UntrustedPolicy::Allow, |current, policy| {
        match (current, policy) {
            (UntrustedPolicy::Block, _) | (_, UntrustedPolicy::Block) => UntrustedPolicy::Block,
            (UntrustedPolicy::Approve, _) | (_, UntrustedPolicy::Approve) => {
                UntrustedPolicy::Approve
            }
            _ => UntrustedPolicy::Allow,
        }
    })
}

/// The `trust_required` denial message: the run, its goal, the origins of the
/// untrusted content, the sink, the policy and the way out. Never a payload,
/// prompt or header (ADR 0016 D4).
fn denial_reason(
    caller: &run::RunTaint,
    tainted: &[run::RunTaint],
    host_call: &str,
    policy: UntrustedPolicy,
    scope: &str,
) -> String {
    let guidance = match policy {
        UntrustedPolicy::Block => {
            "declassify the value with trust(run, value, reason), or run with \
             AgentSpec.untrusted = allow"
        }
        UntrustedPolicy::Approve => {
            "declassify the value with trust(run, value, reason), or attach an approver \
             (without one the decision is a deny)"
        }
        // The `allow` policy proceeds instead of denying; the arm keeps the
        // match exhaustive without inventing a second denial path.
        UntrustedPolicy::Allow => "run with AgentSpec.untrusted = allow",
    };
    format!(
        "trust_required: run '{}' (goal '{}') holds untrusted content from {} and policy '{}' \
         refuses the sink '{}'{}; {}",
        caller.run_id,
        caller.goal,
        origins_text(tainted),
        policy_name(policy),
        host_call,
        scope,
        guidance
    )
}

fn origins_text(tainted: &[run::RunTaint]) -> String {
    let mut origins: Vec<&str> = tainted
        .iter()
        .flat_map(|state| state.untrusted_origins.iter().map(String::as_str))
        .collect();
    origins.sort_unstable();
    origins.dedup();
    if origins.is_empty() {
        "an untracked origin".to_string()
    } else {
        origins.join(", ")
    }
}

/// The policy's spelling in `AgentSpec.untrusted`.
pub(crate) fn policy_name(policy: UntrustedPolicy) -> &'static str {
    match policy {
        UntrustedPolicy::Approve => "approve",
        UntrustedPolicy::Block => "block",
        UntrustedPolicy::Allow => "allow",
    }
}

// ── ledger mutations + journal ───────────────────────────────────────────

/// `untrusted(run, value, origin)`: records untrusted provenance and returns
/// the ledger entry (the value itself is returned unchanged by the host).
pub(crate) fn mark_untrusted(
    run_handle: i64,
    value: &str,
    origin: &str,
) -> Result<Entry, AgentError> {
    let tag = validate_tag(origin, "origin")?;
    let entry = run::with_run(run_handle, |state| state.taint.mark_untrusted(value, &tag))?;
    let input = format!("untrusted\0{tag}\0{value}");
    journal_provenance(run_handle, &input, &entry.digest, tag)?;
    Ok(entry)
}

/// `trust(run, value, reason)`: declassifies one digest. The reason is
/// mandatory — an unattributed declassification is exactly the audit gap this
/// mechanism exists to close.
pub(crate) fn declassify(run_handle: i64, value: &str, reason: &str) -> Result<Entry, AgentError> {
    let tag = validate_tag(reason, "reason")?;
    let entry = run::with_run(run_handle, |state| state.taint.declassify(value, &tag))?;
    let input = format!("trust\0{tag}\0{value}");
    journal_provenance(run_handle, &input, &entry.digest, tag)?;
    Ok(entry)
}

/// Records the provenance of every message the run is about to send to the
/// model: `user` and `model` content is trusted, a tool result is untrusted.
///
/// Observation is monotone (it can never downgrade an untrusted digest) and is
/// deliberately *not* journaled per message: the transcript is re-observed on
/// every turn, and the durable record of a tool result is the tool step that
/// produced it. The explicit `untrusted`/`trust` calls and the gated decisions
/// are the journaled provenance.
pub(crate) fn observe_transcript(run_handle: i64, messages: &[Message]) -> Result<(), AgentError> {
    run::with_run(run_handle, |state| {
        for message in messages {
            match message.role {
                "tool" => {
                    let origin = origin_tool(message.tool_name.as_deref().unwrap_or("unknown"));
                    state.taint.mark_untrusted(&message.content, &origin);
                }
                "user" => {
                    state.taint.observe_trusted(&message.content, ORIGIN_USER);
                }
                _ => {
                    state.taint.observe_trusted(&message.content, ORIGIN_MODEL);
                }
            }
        }
    })
}

/// A tag (origin or reason) must be non-empty, printable and bounded: it
/// becomes journal attribution and denial text.
fn validate_tag(tag: &str, what: &str) -> Result<String, AgentError> {
    if tag.trim().is_empty() {
        return Err(AgentError::Taint(format!(
            "{what} must not be empty: provenance and declassification are only auditable when \
             they are attributable"
        )));
    }
    if tag.chars().any(|character| character.is_control()) {
        return Err(AgentError::Taint(format!(
            "{what} must not contain control characters"
        )));
    }
    Ok(tag.chars().take(MAX_TAG_LEN).collect())
}

/// Journals one `taint` record, returning without appending when the step is
/// already recorded (a replay never duplicates a provenance record).
fn journal_provenance(
    run_handle: i64,
    input: &str,
    digest: &str,
    attribution: String,
) -> Result<(), AgentError> {
    match replay::resolve(run_handle, Kind::Taint, input)? {
        Resolved::Recorded(_) => Ok(()),
        Resolved::Fresh(token) => replay::commit(
            run_handle,
            &token,
            Some(input),
            digest,
            StepUsage::default(),
            -1,
            Some(attribution),
        ),
    }
}

/// Journals one gated decision (`allow`/`deny`) under `taint_decision`.
fn journal_decision(
    caller: &run::RunTaint,
    host_call: &str,
    decision: &str,
    attribution: Option<String>,
) -> Result<(), AgentError> {
    let input = format!("gated\0{host_call}\0{}", policy_name(caller.policy));
    match replay::resolve(caller.handle, Kind::TaintDecision, &input)? {
        Resolved::Recorded(_) => Ok(()),
        Resolved::Fresh(token) => replay::commit(
            caller.handle,
            &token,
            Some(&input),
            decision,
            StepUsage::default(),
            -1,
            attribution,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_is_monotone_and_trust_declassifies_one_digest() {
        let mut ledger = Ledger::default();
        let entry = ledger.mark_untrusted("payload", "external:web");
        assert!(!entry.trusted);

        // A trusted observation of the same digest never launders it.
        let observed = ledger.observe_trusted("payload", ORIGIN_USER);
        assert!(!observed.trusted);
        assert!(ledger.holds_untrusted());

        // A derived value is a different digest and stays trusted on its own.
        ledger.observe_trusted("payload transformed", ORIGIN_MODEL);
        assert_eq!(ledger.len(), 2);

        let declassified = ledger.declassify("payload", "reviewed by an operator");
        assert!(declassified.trusted);
        assert_eq!(declassified.reason, "reviewed by an operator");
        assert_eq!(declassified.origin, "external:web");
        assert!(!ledger.holds_untrusted());
    }

    #[test]
    fn a_tool_result_defaults_to_untrusted_after_a_trusted_observation() {
        let mut ledger = Ledger::default();
        assert!(ledger.observe_trusted("same bytes", ORIGIN_MODEL).trusted);
        // The untrusted mark wins regardless of the order it arrives in.
        ledger.mark_untrusted("same bytes", &origin_tool("fetch"));
        assert!(ledger.holds_untrusted());
        assert_eq!(ledger.untrusted_origins(), vec!["tool:fetch".to_string()]);
    }

    /// The sink set is the catalog's, not a list maintained beside it: every
    /// catalog entry classified `sink` resolves as a sink here, and no entry
    /// the catalog leaves unclassified is gated. The documented write-side
    /// namespaces must be inside it.
    #[test]
    fn the_sink_set_is_the_catalog_classification() {
        let catalog = spectra_contract::catalog();
        for entry in &catalog.entry {
            assert_eq!(
                is_sink(&entry.binding),
                entry.sink,
                "{} sink classification must follow the catalog",
                entry.path
            );
        }
        for namespace in [
            "spectra.std.fs.",
            "spectra.std.env.",
            "spectra.api.db.migrate.",
            "spectra.std.collections.",
        ] {
            assert!(
                catalog
                    .entry
                    .iter()
                    .any(|entry| entry.sink && entry.binding.starts_with(namespace)),
                "the write-side namespace {namespace} must be classified"
            );
        }
        assert!(is_sink("spectra.std.fs.fs_write"));
        assert!(!is_sink("spectra.std.fs.fs_read"));
    }

    #[test]
    fn the_scope_registry_reads_a_declared_argument() {
        let table = unsafe { abi::alloc_string("migrations") };
        assert_eq!(
            scope_value(
                "spectra.api.db.migrate.apply_sqlite",
                "table",
                &[0, table]
            ),
            Some("migrations".to_string())
        );
        // A key with no extractor (the HTTP method inside a Request handle)
        // and an unknown host call both defer to the name-level gate.
        assert_eq!(
            scope_value("spectra.api.client.request", "method", &[1, 2]),
            None
        );
    }

    #[test]
    fn the_strictest_policy_on_the_chain_wins() {
        use UntrustedPolicy::{Allow, Approve, Block};
        assert_eq!(strictest([Allow].into_iter()), Allow);
        assert_eq!(strictest([Allow, Approve].into_iter()), Approve);
        assert_eq!(strictest([Allow, Approve, Block].into_iter()), Block);
        assert_eq!(strictest([Block, Allow].into_iter()), Block);
    }

    // ── policy matrix through the real dispatch seam ─────────────────────

    use std::path::PathBuf;

    use spectra_runtime::ffi::{HOST_STATUS_DENIED, HOST_STATUS_SUCCESS};

    use crate::journal::Journal;
    use crate::run::{alloc_run, take_run};
    use crate::spec::AgentSpec;

    /// One live run, entered on the run context, journaled in a fresh
    /// directory so the gated decisions can be read back from disk.
    struct Fixture {
        handle: i64,
        dir: PathBuf,
        run_id: String,
    }

    impl Fixture {
        fn new(name: &str, policy: &str) -> Self {
            let run_id = format!("taint-{name}");
            let dir = std::env::temp_dir().join(format!("spectra-taint-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let json = format!(
                r#"{{"goal":"taint-{name}","model":"mock/echo","allow":["spectra.std.fs"],"untrusted":"{policy}"}}"#
            );
            let spec = AgentSpec::parse(&json).expect("spec");
            let journal = Journal::open(&run_id, &dir.to_string_lossy(), false).expect("journal");
            let handle = alloc_run(spec, run_id.clone(), Some(journal)).expect("alloc");
            Self { handle, dir, run_id }
        }

        fn untrusted(&self, value: &str, origin: &str) {
            mark_untrusted(self.handle, value, origin).expect("untrusted");
        }

        /// Dispatches one host call the way the run performs its own work: the
        /// sink is reached from inside the run's dynamic extent (a tool
        /// wrapper invoked by `act`/`tool_call`), which is where the policy
        /// seam sees the run on the chain.
        fn dispatch_in_run(&self, host_call: &str, args: &[SpectraHostValue]) -> i32 {
            run::in_run_scope(self.handle, || dispatch(host_call, args))
        }

        /// The journal records, in step order.
        fn records(&self) -> Vec<serde_json::Value> {
            let path = self.dir.join(crate::journal::file_name(&self.run_id));
            std::fs::read_to_string(path)
                .expect("the fixture journals every decision")
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| serde_json::from_str(line).expect("record"))
                .collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            take_run(self.handle).expect("end");
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Drives one host call through the runtime's single generic dispatch
    /// entrypoint, exactly as compiled code does.
    fn dispatch(host_call: &str, args: &[SpectraHostValue]) -> i32 {
        let mut results = [0 as SpectraHostValue; 1];
        spectra_runtime::ffi::spectra_rt_host_invoke(
            host_call.as_ptr(),
            host_call.len(),
            args.as_ptr(),
            args.len(),
            results.as_mut_ptr(),
            results.len(),
        )
    }

    fn sink_args(path: &str) -> [SpectraHostValue; 2] {
        [
            unsafe { abi::alloc_string(path) },
            unsafe { abi::alloc_string("written") },
        ]
    }

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        spectra_runtime::register();
        crate::policy::install();
        crate::approval::set_approver(None);
        guard
    }

    /// `block`: the sink is refused while untrusted content is in the run, and
    /// the denial is durable with a `trust_required` reason.
    #[test]
    fn block_denies_a_sink_and_journals_the_denial() {
        let _guard = setup();
        let fixture = Fixture::new("block", "block");
        fixture.untrusted("payload from the web", "external:web");
        let target = fixture.dir.join("blocked.txt");
        let status = fixture.dispatch_in_run(
            "spectra.std.fs.fs_write",
            &sink_args(&target.to_string_lossy()),
        );
        assert_eq!(status, HOST_STATUS_DENIED, "block must refuse the sink");
        assert!(
            !target.exists(),
            "the denied host call must never reach the host"
        );

        let denial = fixture
            .records()
            .into_iter()
            .find(|record| record["kind"] == "taint_decision")
            .expect("the denial is journaled");
        assert_eq!(denial["output"], "deny");
        let attribution = denial["attribution"].as_str().expect("attribution");
        assert!(attribution.contains("trust_required"), "{attribution}");
        assert!(attribution.contains("taint-block"), "{attribution}");
        assert!(attribution.contains("external:web"), "{attribution}");
        assert!(
            !attribution.contains("payload from the web"),
            "the denial text must not carry the payload: {attribution}"
        );
    }

    /// `allow`: the sink proceeds and the decision is journaled.
    #[test]
    fn allow_proceeds_and_journals_the_decision() {
        let _guard = setup();
        let fixture = Fixture::new("allow", "allow");
        fixture.untrusted("payload from the web", "external:web");
        let target = fixture.dir.join("allowed.txt");
        let status = fixture.dispatch_in_run(
            "spectra.std.fs.fs_write",
            &sink_args(&target.to_string_lossy()),
        );
        assert_eq!(status, HOST_STATUS_SUCCESS, "the policy allowed the sink");
        assert!(target.exists(), "the host call ran");

        let decision = fixture
            .records()
            .into_iter()
            .find(|record| record["kind"] == "taint_decision")
            .expect("the decision is journaled");
        assert_eq!(decision["output"], "allow");
    }

    /// `approve` (the default): no approver means fail closed; an attached
    /// approver that allows the action unblocks the sink and the decision is
    /// journaled as an approval.
    #[test]
    fn approve_fails_closed_without_an_approver_and_proceeds_with_one() {
        struct AllowOnce;
        impl crate::approval::Approver for AllowOnce {
            fn decide(&self, _request: &crate::approval::ApprovalRequest) -> approval::Decision {
                approval::Decision::AllowOnce {
                    by: "test-approver".to_string(),
                }
            }
        }

        let _guard = setup();
        let fixture = Fixture::new("approve", "approve");
        fixture.untrusted("payload from the web", "external:web");
        let target = fixture.dir.join("approved.txt");
        let denied = fixture.dispatch_in_run(
            "spectra.std.fs.fs_write",
            &sink_args(&target.to_string_lossy()),
        );
        assert_eq!(
            denied, HOST_STATUS_DENIED,
            "an unattended run must fail closed"
        );
        assert!(!target.exists());

        crate::approval::set_approver(Some(std::sync::Arc::new(AllowOnce)));
        let allowed = fixture.dispatch_in_run(
            "spectra.std.fs.fs_write",
            &sink_args(&target.to_string_lossy()),
        );
        assert_eq!(allowed, HOST_STATUS_SUCCESS, "the approver allowed it");
        assert!(target.exists());

        let records = fixture.records();
        // The unattended phase is a recorded denial...
        assert!(records
            .iter()
            .any(|record| record["kind"] == "approval" && record["output"] == "false"));
        // ...and the attached approver's allow-once decision is attributable.
        let approval_record = records
            .iter()
            .find(|record| record["kind"] == "approval" && record["output"] == "true")
            .expect("the approval is journaled");
        assert_eq!(
            approval_record["attribution"], "allow-once by test-approver",
            "the approver is attributable"
        );
    }

    /// `trust(run, value, reason)` declassifies exactly that value, which is
    /// what lets a `block` run reach the sink again.
    #[test]
    fn a_declassification_unblocks_a_blocked_sink_and_is_audited() {
        let _guard = setup();
        let fixture = Fixture::new("trust", "block");
        fixture.untrusted("reviewed content", "external:web");
        let target = fixture.dir.join("reviewed.txt");
        assert_eq!(
            fixture.dispatch_in_run(
                "spectra.std.fs.fs_write",
                &sink_args(&target.to_string_lossy())
            ),
            HOST_STATUS_DENIED
        );

        let entry = declassify(fixture.handle, "reviewed content", "reviewed by an operator")
            .expect("declassify");
        assert!(entry.trusted);
        assert_eq!(
            fixture.dispatch_in_run(
                "spectra.std.fs.fs_write",
                &sink_args(&target.to_string_lossy())
            ),
            HOST_STATUS_SUCCESS,
            "the declassified value no longer blocks the sink"
        );

        let audit = fixture
            .records()
            .into_iter()
            .find(|record| {
                record["kind"] == "taint"
                    && record["attribution"] == "reviewed by an operator"
            })
            .expect("the declassification is audited");
        assert_eq!(
            audit["output"], entry.digest,
            "the audit record names the digest it declassified"
        );
    }

    /// A declassification without a reason is refused: attribution is the
    /// point of the ledger.
    #[test]
    fn a_declassification_without_a_reason_is_refused() {
        let _guard = setup();
        let fixture = Fixture::new("noreason", "block");
        let error = declassify(fixture.handle, "value", "   ").expect_err("reason required");
        assert_eq!(error.kind(), "taint_error", "{error}");
    }

    /// Only catalog sinks are gated: an untrusted run still reads files.
    #[test]
    fn a_non_sink_host_call_is_not_gated() {
        let _guard = setup();
        let fixture = Fixture::new("nonsink", "block");
        fixture.untrusted("payload from the web", "external:web");
        let path = fixture.dir.join("source.txt");
        std::fs::create_dir_all(&fixture.dir).expect("dir");
        std::fs::write(&path, "readable").expect("write");
        let status = fixture.dispatch_in_run(
            "spectra.std.fs.fs_read",
            &[unsafe { abi::alloc_string(&path.to_string_lossy()) }],
        );
        assert_eq!(
            status, HOST_STATUS_SUCCESS,
            "a read is not a sink and must not be gated"
        );
    }

    /// A tool result enters the transcript untrusted; the model turn does not
    /// launder it, so the sink is still gated on the next step.
    #[test]
    fn a_tool_result_taints_the_run_through_the_transcript() {
        let _guard = setup();
        let fixture = Fixture::new("transcript", "block");
        let messages = vec![
            Message::user("do the task"),
            Message::tool("fetch", "remote body"),
        ];
        observe_transcript(fixture.handle, &messages).expect("observe");
        let state = run::taint_for_raw_id(fixture.handle as u64).expect("live run");
        assert!(state.holds_untrusted);
        assert_eq!(state.untrusted_origins, vec!["tool:fetch".to_string()]);

        // Re-observing the same transcript (the next turn) does not downgrade.
        observe_transcript(fixture.handle, &messages).expect("observe again");
        assert!(run::taint_for_raw_id(fixture.handle as u64)
            .expect("live run")
            .holds_untrusted);
    }
}
