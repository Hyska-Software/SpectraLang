//! Durable run journal: append-only records, one file per run (R-3217 T1).
//!
//! ADR 0018 freezes the record shape (`run`, `step`, `kind`, input digest,
//! output digest, idempotency key, seed, usage, timestamp) and the durability
//! rule: the writer flushes before returning from any effect-bearing call, so
//! a crash cannot lose the record of an effect that already happened.
//!
//! The journal is JSON Lines: one `serde_json` object per line. Digests are
//! always recorded; the request payload (prompt, tool arguments) is recorded
//! only when the spec's `journal_payloads` is set. The *output* an effect
//! produced is always recorded, because replay returns it instead of
//! re-executing the effect — a digest alone could not be returned to the
//! caller, and re-executing to recover it is exactly the duplication I3
//! forbids. `attribution` carries the human-facing payload a replayed step
//! needs (an approval's who/when, an assertion's message).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::digest;
use crate::error::AgentError;

/// Directory used when the spec's `journal` field is absent. An explicit empty
/// string disables journaling (it is the field's long-standing default, so
/// every pre-R-3217 spec stays journal-free).
pub(crate) const DEFAULT_DIR: &str = ".spectra/journal";

/// Tokens and cost attributed to one step, exactly as the provider reported
/// them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StepUsage {
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_micros: u64,
}

/// One journal record. The first nine fields are the ADR-frozen shape; the
/// last three are the documented extensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    pub run: String,
    pub step: u64,
    pub kind: String,
    pub input_digest: String,
    pub output_digest: String,
    pub idempotency_key: String,
    pub seed: i64,
    pub usage: StepUsage,
    pub timestamp: u64,
    /// The effect's result, always recorded: replay returns it verbatim.
    pub output: String,
    /// The request payload; present only when payload capture is opted in.
    pub input: Option<String>,
    /// Who decided / which message failed, for a replayed decision.
    pub attribution: Option<String>,
}

impl Record {
    fn to_json(&self) -> Value {
        let mut object = json!({
            "run": self.run,
            "step": self.step,
            "kind": self.kind,
            "input_digest": self.input_digest,
            "output_digest": self.output_digest,
            "idempotency_key": self.idempotency_key,
            "seed": self.seed,
            "usage": {
                "tokens_in": self.usage.tokens_in,
                "tokens_out": self.usage.tokens_out,
                "cost_micros": self.usage.cost_micros,
            },
            "timestamp": self.timestamp,
            "output": self.output,
        });
        if let Some(input) = &self.input {
            object["input"] = json!(input);
        }
        if let Some(attribution) = &self.attribution {
            object["attribution"] = json!(attribution);
        }
        object
    }

    pub(crate) fn from_json(value: &Value) -> Result<Self, AgentError> {
        let object = value
            .as_object()
            .ok_or_else(|| AgentError::Journal("record is not a JSON object".to_string()))?;
        let run = text(object, "run")?;
        let step = number(object, "step")?;
        let kind = text(object, "kind")?;
        let input_digest = text(object, "input_digest")?;
        let output_digest = text(object, "output_digest")?;
        let idempotency_key = text(object, "idempotency_key")?;
        let seed = signed(object, "seed")?;
        let timestamp = number(object, "timestamp")?;
        let output = text(object, "output")?;
        let usage = object
            .get("usage")
            .and_then(Value::as_object)
            .map(|usage| {
                Ok::<_, AgentError>(StepUsage {
                    tokens_in: non_negative(usage, "tokens_in")?,
                    tokens_out: non_negative(usage, "tokens_out")?,
                    cost_micros: non_negative(usage, "cost_micros")?,
                })
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            run,
            step,
            kind,
            input_digest,
            output_digest,
            idempotency_key,
            seed,
            usage,
            timestamp,
            output,
            input: object.get("input").and_then(Value::as_str).map(str::to_string),
            attribution: object
                .get("attribution")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}

fn text(object: &serde_json::Map<String, Value>, key: &str) -> Result<String, AgentError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| AgentError::Journal(format!("journal record is missing '{key}'")))
}

fn number(object: &serde_json::Map<String, Value>, key: &str) -> Result<u64, AgentError> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| AgentError::Journal(format!("journal record is missing '{key}'")))
}

fn non_negative(object: &serde_json::Map<String, Value>, key: &str) -> Result<u64, AgentError> {
    match object.get(key) {
        Some(value) => value
            .as_u64()
            .ok_or_else(|| AgentError::Journal(format!("journal usage field '{key}' is invalid"))),
        None => Ok(0),
    }
}

fn signed(object: &serde_json::Map<String, Value>, key: &str) -> Result<i64, AgentError> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| AgentError::Journal(format!("journal record is missing '{key}'")))
}

/// Milliseconds since the Unix epoch; 0 when the clock is unreadable.
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// A fresh run identity: process, wall clock and a process-local counter, so
/// two runs in the same millisecond cannot collide.
pub(crate) fn new_run_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "run-{}-{}-{}",
        std::process::id(),
        now_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Maps a run id onto a file name. Run ids are author-supplied, so anything
/// outside the conservative safe set is replaced; a run id that had to be
/// rewritten keeps a digest suffix so two distinct ids cannot share a file.
pub(crate) fn file_name(run: &str) -> String {
    if run.is_empty() {
        // Never reachable from a real run (the id is generated or supplied),
        // but the mapping stays total and must not collide with a run named
        // "run".
        return "run-unnamed.jsonl".to_string();
    }
    let safe: String = run
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let safe = if safe.is_empty() {
        "run".to_string()
    } else {
        safe
    };
    if safe == run {
        format!("{safe}.jsonl")
    } else {
        format!("{safe}-{}.jsonl", &digest::of(run)[..16])
    }
}

/// One run's journal: the records already on disk plus the writer for new
/// ones.
#[derive(Debug)]
pub(crate) struct Journal {
    run: String,
    dir: PathBuf,
    payloads: bool,
    /// Records by step, loaded at open and extended by [`Journal::append`].
    records: BTreeMap<u64, Record>,
    /// Next step number this run will use.
    pub(crate) next_step: u64,
    /// Whether the journal was found on disk: the run replays recorded steps.
    replaying: bool,
    file: Option<File>,
}

impl Journal {
    /// Opens (or resumes) the journal for `run` under `dir`.
    ///
    /// A journal found on disk puts the run in replay mode. A missing or empty
    /// file starts a new run that will append. Reading a malformed record is a
    /// typed error: silently skipping it would defeat the durability promise.
    pub(crate) fn open(run: &str, dir: &str, payloads: bool) -> Result<Self, AgentError> {
        let dir = PathBuf::from(dir);
        let path = dir.join(file_name(run));
        let mut records = BTreeMap::new();
        let mut replaying = false;
        if path.exists() {
            let contents = fs::read_to_string(&path).map_err(|error| {
                AgentError::Journal(format!("cannot read journal {}: {error}", path.display()))
            })?;
            for (index, line) in contents.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let value: Value = serde_json::from_str(line).map_err(|error| {
                    AgentError::Journal(format!(
                        "journal {} line {} is not valid JSON: {error}",
                        path.display(),
                        index + 1
                    ))
                })?;
                let record = Record::from_json(&value)?;
                records.insert(record.step, record);
            }
            replaying = true;
        }
        // Every run re-derives its effect sequence from step 0: a resumed run
        // must compute the same step key as the recorded run so the record
        // matches, and a fresh run starts at the beginning too.
        Ok(Self {
            run: run.to_string(),
            dir,
            payloads,
            records,
            next_step: 0,
            replaying,
            file: None,
        })
    }

    /// Whether this run resumed an existing journal.
    pub(crate) fn replaying(&self) -> bool {
        self.replaying
    }

    /// Whether request payloads are captured.
    pub(crate) fn captures_payloads(&self) -> bool {
        self.payloads
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.dir.join(file_name(&self.run))
    }

    pub(crate) fn get(&self, step: u64) -> Option<&Record> {
        self.records.get(&step)
    }

    /// Number of recorded (or loaded) steps; test/support accessor.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    /// Records in step order; test/support accessor.
    #[cfg(test)]
    pub(crate) fn ordered(&self) -> impl Iterator<Item = &Record> {
        self.records.values()
    }

    /// Appends one record and flushes it before returning.
    ///
    /// The flush is the durability contract, not a best-effort log write: a
    /// crash after this call returns cannot lose the record of an effect that
    /// already happened.
    pub(crate) fn append(&mut self, record: Record) -> Result<(), AgentError> {
        if self.file.is_none() {
            fs::create_dir_all(&self.dir).map_err(|error| {
                AgentError::Journal(format!(
                    "cannot create journal directory {}: {error}",
                    self.dir.display()
                ))
            })?;
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.path())
                .map_err(|error| {
                    AgentError::Journal(format!(
                        "cannot open journal {}: {error}",
                        self.path().display()
                    ))
                })?;
            self.file = Some(file);
        }
        let line = format!("{}\n", record.to_json());
        let path = self.path();
        let step = record.step;
        let file = self.file.as_mut().expect("journal file opened above");
        file.write_all(line.as_bytes())
            .map_err(|error| AgentError::Journal(format!("cannot write journal {}: {error}", path.display())))?;
        file.flush().map_err(|error| {
            AgentError::Journal(format!("cannot flush journal {}: {error}", path.display()))
        })?;
        self.records.insert(step, record);
        // The next step is always past every recorded one, whichever path
        // inserted the record.
        self.next_step = self.next_step.max(step + 1);
        Ok(())
    }
}

/// Reads the run id from a journal file path, if it parses (test/support
/// helper used by the validators).
#[allow(dead_code)]
pub(crate) fn run_id_of(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    let first = contents.lines().next()?;
    let value: Value = serde_json::from_str(first).ok()?;
    value.get("run").and_then(Value::as_str).map(str::to_string)
}

/// The journal file path for a run under `dir`, before it exists.
pub(crate) fn file_path(dir: &Path, run: &str) -> PathBuf {
    dir.join(file_name(run))
}

/// Reads a run's journal file, oldest step first.
///
/// A missing file is not an error: "the run recorded nothing" is an
/// observation the eval harness grades, not a failure of the reader. A
/// malformed line is an error, matching [`Journal::open`]: silently skipping
/// it would defeat the durability promise.
pub(crate) fn read_records(dir: &Path, run: &str) -> Result<Vec<Record>, AgentError> {
    let path = file_path(dir, run);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(&path).map_err(|error| {
        AgentError::Journal(format!("cannot read journal {}: {error}", path.display()))
    })?;
    let mut records = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).map_err(|error| {
            AgentError::Journal(format!(
                "journal {} line {} is not valid JSON: {error}",
                path.display(),
                index + 1
            ))
        })?;
        records.push(Record::from_json(&value)?);
    }
    records.sort_by_key(|record| record.step);
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(step: u64, kind: &str, output: &str) -> Record {
        Record {
            run: "run-1".to_string(),
            step,
            kind: kind.to_string(),
            input_digest: digest::of(&format!("in-{step}")),
            output_digest: digest::of(output),
            idempotency_key: digest::step_key("run-1", step, &digest::of(&format!("in-{step}"))),
            seed: 3,
            usage: StepUsage {
                tokens_in: 1,
                tokens_out: 2,
                cost_micros: 5,
            },
            timestamp: now_ms(),
            output: output.to_string(),
            input: None,
            attribution: None,
        }
    }

    #[test]
    fn appends_flush_and_reload_in_step_order() {
        let dir = std::env::temp_dir().join(format!("spectra-journal-{}", new_run_id()));
        let dir_text = dir.to_string_lossy().to_string();
        let mut journal = Journal::open("run-1", &dir_text, false).expect("open");
        assert!(!journal.replaying());
        journal.append(record(1, "tool", "second")).expect("append");
        journal.append(record(0, "model", "first")).expect("append");
        assert_eq!(journal.next_step, 2);

        // The file is on disk and complete before the writer is dropped.
        let reloaded = Journal::open("run-1", &dir_text, false).expect("reload");
        assert!(reloaded.replaying());
        assert_eq!(reloaded.len(), 2);
        // A resumed run re-derives its sequence from step 0 and matches the
        // recorded steps by key; the counter is the run's own.
        assert_eq!(reloaded.next_step, 0);
        let outputs: Vec<&str> = reloaded.ordered().map(|record| record.output.as_str()).collect();
        assert_eq!(outputs, ["first", "second"]);
        assert_eq!(reloaded.get(0).expect("step 0").kind, "model");
        assert_eq!(reloaded.get(0).expect("step 0").usage.tokens_out, 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn payloads_are_captured_only_when_opted_in() {
        let dir = std::env::temp_dir().join(format!("spectra-journal-{}", new_run_id()));
        let dir_text = dir.to_string_lossy().to_string();
        let mut journal = Journal::open("run-2", &dir_text, true).expect("open");
        let mut capture = record(0, "model", "answer");
        capture.input = Some("prompt".to_string());
        journal.append(capture).expect("append");
        let contents = fs::read_to_string(journal.path()).expect("read");
        assert!(contents.contains("\"input\":\"prompt\""), "{contents}");

        // Without the opt-in the request payload never reaches the file; the
        // recorded output still does.
        let mut default_journal = Journal::open("run-3", &dir_text, false).expect("open");
        default_journal.append(record(0, "model", "answer")).expect("append");
        let contents = fs::read_to_string(default_journal.path()).expect("read");
        assert!(!contents.contains("\"input\""), "{contents}");
        assert!(contents.contains("\"output\":\"answer\""), "{contents}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_malformed_record_fails_closed() {
        let dir = std::env::temp_dir().join(format!("spectra-journal-{}", new_run_id()));
        fs::create_dir_all(&dir).expect("create");
        fs::write(dir.join("run-4.jsonl"), "{not json}\n").expect("write");
        let error = Journal::open("run-4", &dir.to_string_lossy(), false).expect_err("malformed");
        assert_eq!(error.kind(), "journal_error");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_names_are_path_safe_and_distinct() {
        assert_eq!(file_name("run-1"), "run-1.jsonl");
        assert_eq!(file_name(""), "run-unnamed.jsonl");
        let a = file_name("a/b");
        let b = file_name("a_b");
        assert_ne!(a, b, "distinct run ids must not share a file");
        assert!(a.ends_with(".jsonl") && !a.contains('/'), "{a}");
    }
}
