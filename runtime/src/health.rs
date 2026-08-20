//! Runtime health registry used by the API server's liveness/readiness probes.
//!
//! Checks are evaluated by a dedicated worker and HTTP handlers only read the
//! last atomically published snapshot.  A slow dependency therefore cannot
//! block the request thread.

use crate::tracing::{self, SpanKind, SpanStatus};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthState {
    Healthy,
    Degraded,
    Unavailable,
    Starting,
}

impl HealthState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
            Self::Starting => "starting",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthCategory {
    Database,
    Cache,
    ExternalService,
    Custom,
}

impl HealthCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Cache => "cache",
            Self::ExternalService => "external_service",
            Self::Custom => "custom",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HealthError {
    InvalidName,
    DuplicateName,
    InvalidTimeout,
    ShuttingDown,
}

impl fmt::Display for HealthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::InvalidName => "health check name is empty or invalid",
                Self::DuplicateName => "health check name is already registered",
                Self::InvalidTimeout => "health check timeout must be greater than zero",
                Self::ShuttingDown => "health registry is shutting down",
            }
        )
    }
}
impl std::error::Error for HealthError {}

type CheckFn = Arc<dyn Fn() -> Result<(), String> + Send + Sync + 'static>;

struct CheckSpec {
    category: HealthCategory,
    timeout: Duration,
    required: bool,
    check: CheckFn,
}

#[derive(Clone, Debug)]
pub struct CheckSnapshot {
    pub status: String,
    pub required: bool,
    pub category: String,
    pub duration_ms: u64,
    pub evaluated_at_ms: u128,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HealthSnapshot {
    pub liveness: HealthState,
    pub readiness: HealthState,
    pub startup: HealthState,
    pub checks: BTreeMap<String, CheckSnapshot>,
    pub generated_at_ms: u128,
}

enum Command {
    Refresh(mpsc::SyncSender<()>),
    Shutdown(mpsc::SyncSender<()>),
}

struct Inner {
    checks: Mutex<BTreeMap<String, CheckSpec>>,
    snapshot: RwLock<HealthSnapshot>,
    startup: Mutex<HealthState>,
    commands: mpsc::SyncSender<Command>,
    shutdown: AtomicBool,
    worker: Mutex<Option<JoinHandle<()>>>,
    stale_after: Mutex<Duration>,
}

#[derive(Clone)]
pub struct HealthRegistry {
    inner: Arc<Inner>,
}

impl HealthRegistry {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(64);
        let initial = HealthSnapshot {
            liveness: HealthState::Healthy,
            readiness: HealthState::Healthy,
            startup: HealthState::Starting,
            checks: BTreeMap::new(),
            generated_at_ms: now_ms(),
        };
        let inner = Arc::new_cyclic(|_weak| Inner {
            checks: Mutex::new(BTreeMap::new()),
            snapshot: RwLock::new(initial),
            startup: Mutex::new(HealthState::Starting),
            commands: tx.clone(),
            shutdown: AtomicBool::new(false),
            worker: Mutex::new(None),
            stale_after: Mutex::new(Duration::from_secs(30)),
        });
        let thread_inner = Arc::downgrade(&inner);
        let worker = thread::Builder::new()
            .name("spectra-health-evaluator".into())
            .spawn(move || {
                while let Ok(command) = rx.recv() {
                    let Some(inner) = thread_inner.upgrade() else {
                        break;
                    };
                    match command {
                        Command::Refresh(done) => {
                            evaluate(&inner);
                            let _ = done.send(());
                        }
                        Command::Shutdown(done) => {
                            let _ = done.send(());
                            break;
                        }
                    }
                }
            })
            .expect("health evaluator thread must start");
        *inner.worker.lock().unwrap() = Some(worker);
        Self { inner }
    }

    pub fn register_check<F>(
        &self,
        name: impl Into<String>,
        category: HealthCategory,
        timeout: Duration,
        required: bool,
        check: F,
    ) -> Result<(), HealthError>
    where
        F: Fn() -> Result<(), String> + Send + Sync + 'static,
    {
        let name = name.into();
        if name.trim().is_empty() || name.len() > 128 {
            return Err(HealthError::InvalidName);
        }
        if timeout.is_zero() {
            return Err(HealthError::InvalidTimeout);
        }
        if self.inner.shutdown.load(Ordering::SeqCst) {
            return Err(HealthError::ShuttingDown);
        }
        let mut checks = self.inner.checks.lock().unwrap();
        if checks.contains_key(&name) {
            return Err(HealthError::DuplicateName);
        }
        checks.insert(
            name,
            CheckSpec {
                category,
                timeout,
                required,
                check: Arc::new(check),
            },
        );
        Ok(())
    }

    pub fn remove_check(&self, name: &str) -> bool {
        self.inner.checks.lock().unwrap().remove(name).is_some()
    }
    pub fn set_stale_after(&self, duration: Duration) -> Result<(), HealthError> {
        if duration.is_zero() {
            return Err(HealthError::InvalidTimeout);
        }
        *self.inner.stale_after.lock().unwrap() = duration;
        Ok(())
    }
    pub fn set_startup_complete(&self) {
        *self.inner.startup.lock().unwrap() = HealthState::Healthy;
    }
    pub fn set_startup_failed(&self, _reason: impl Into<String>) {
        *self.inner.startup.lock().unwrap() = HealthState::Unavailable;
    }

    pub fn refresh(&self) -> Result<(), HealthError> {
        if self.inner.shutdown.load(Ordering::SeqCst) {
            return Err(HealthError::ShuttingDown);
        }
        let (done_tx, done_rx) = mpsc::sync_channel(0);
        self.inner
            .commands
            .send(Command::Refresh(done_tx))
            .map_err(|_| HealthError::ShuttingDown)?;
        let _ = done_rx.recv_timeout(Duration::from_secs(30));
        Ok(())
    }

    pub fn snapshot(&self) -> HealthSnapshot {
        self.inner.snapshot.read().unwrap().clone()
    }
    pub fn liveness(&self) -> HealthState {
        HealthState::Healthy
    }
    pub fn readiness(&self) -> HealthState {
        self.snapshot().readiness
    }
    pub fn startup(&self) -> HealthState {
        *self.inner.startup.lock().unwrap()
    }

    pub fn shutdown(&self, timeout: Duration) -> bool {
        if self.inner.shutdown.swap(true, Ordering::SeqCst) {
            return true;
        }
        let (done_tx, done_rx) = mpsc::sync_channel(0);
        let _ = self.inner.commands.send(Command::Shutdown(done_tx));
        let ok = done_rx.recv_timeout(timeout).is_ok();
        if ok {
            if let Some(worker) = self.inner.worker.lock().unwrap().take() {
                let _ = worker.join();
            }
        }
        ok
    }

    pub fn json(&self, endpoint: &str) -> String {
        let mut snapshot = self.snapshot();
        snapshot.startup = self.startup();
        let stale_after = *self.inner.stale_after.lock().unwrap();
        snapshot_json(&snapshot, endpoint, stale_after)
    }
}

impl Default for HealthRegistry {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for HealthRegistry {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            let _ = self.shutdown(Duration::from_secs(2));
        }
    }
}

static GLOBAL: OnceLock<HealthRegistry> = OnceLock::new();
pub fn global() -> HealthRegistry {
    GLOBAL.get_or_init(HealthRegistry::new).clone()
}

fn evaluate(inner: &Arc<Inner>) {
    let specs = inner
        .checks
        .lock()
        .unwrap()
        .iter()
        .map(|(name, spec)| {
            (
                name.clone(),
                spec.category,
                spec.timeout,
                spec.required,
                Arc::clone(&spec.check),
            )
        })
        .collect::<Vec<_>>();
    let mut checks = BTreeMap::new();
    for (name, category, timeout, required, check) in specs {
        let started = std::time::Instant::now();
        let (tx, rx) = mpsc::sync_channel(1);
        let _ = thread::Builder::new()
            .name(format!("spectra-health-check-{name}"))
            .spawn(move || {
                let _ = tx.send(check());
            });
        let (status, error) = match rx.recv_timeout(timeout) {
            Ok(Ok(())) => ("pass".to_string(), None),
            Ok(Err(error)) => ("fail".to_string(), Some(sanitize(&error))),
            Err(_) => ("timeout".to_string(), Some("health check timed out".into())),
        };
        let span = tracing::span_start("health.check", SpanKind::Internal).ok();
        if let Some(id) = span {
            let _ = tracing::span_set_attribute(id, "health.check.name", &name);
            let _ = tracing::span_set_attribute(id, "health.check.category", category.as_str());
            let _ = tracing::span_set_attribute_bool(id, "health.check.required", required);
            let _ = tracing::span_set_attribute(id, "health.check.status", &status);
            let _ = tracing::span_set_attribute_int(
                id,
                "health.check.duration_ms",
                started.elapsed().as_millis() as i64,
            );
            let _ = tracing::span_set_status(
                id,
                if status == "pass" {
                    SpanStatus::Ok
                } else {
                    SpanStatus::Error
                },
            );
            let _ = tracing::span_end(id);
        }
        checks.insert(
            name,
            CheckSnapshot {
                status,
                required,
                category: category.as_str().into(),
                duration_ms: started.elapsed().as_millis() as u64,
                evaluated_at_ms: now_ms(),
                error,
            },
        );
    }
    let readiness = if checks.values().any(|c| c.required && c.status != "pass") {
        HealthState::Unavailable
    } else if checks.values().any(|c| c.status != "pass") {
        HealthState::Degraded
    } else {
        HealthState::Healthy
    };
    let startup = *inner.startup.lock().unwrap();
    *inner.snapshot.write().unwrap() = HealthSnapshot {
        liveness: HealthState::Healthy,
        readiness,
        startup,
        checks,
        generated_at_ms: now_ms(),
    };
}

fn sanitize(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    if lower.contains("password") || lower.contains("secret") || lower.contains("://") {
        "health check failed".into()
    } else {
        input.chars().take(256).collect()
    }
}
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
fn snapshot_json(snapshot: &HealthSnapshot, endpoint: &str, stale_after: Duration) -> String {
    let state = match endpoint {
        "healthz" => snapshot.liveness,
        "startupz" => snapshot.startup,
        _ => snapshot.readiness,
    };
    let mut out = format!("{{\"status\":\"{}\",\"checks\":{{", state.as_str());
    for (index, (name, check)) in snapshot.checks.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let stale = now_ms().saturating_sub(check.evaluated_at_ms) > stale_after.as_millis();
        out.push_str(&format!(
            "\"{}\":{{\"status\":\"{}\",\"required\":{},\"duration_ms\":{},\"stale\":{}",
            json_escape(name),
            check.status,
            check.required,
            check.duration_ms,
            stale
        ));
        if let Some(error) = &check.error {
            out.push_str(&format!(",\"error\":\"{}\"", json_escape(error)));
        }
        out.push('}');
    }
    out.push_str("}}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_and_optional_failures_have_distinct_readiness() {
        let registry = HealthRegistry::new();
        registry
            .register_check(
                "required",
                HealthCategory::Custom,
                Duration::from_millis(50),
                true,
                || Err("down".into()),
            )
            .unwrap();
        registry
            .register_check(
                "optional",
                HealthCategory::Custom,
                Duration::from_millis(50),
                false,
                || Err("degraded".into()),
            )
            .unwrap();
        registry.refresh().unwrap();
        assert_eq!(registry.readiness(), HealthState::Unavailable);
        registry.remove_check("required");
        registry.refresh().unwrap();
        assert_eq!(registry.readiness(), HealthState::Degraded);
        assert!(registry.json("readyz").contains("degraded"));
        assert!(registry.shutdown(Duration::from_secs(1)));
    }

    #[test]
    fn startup_state_is_explicit_and_liveness_is_independent() {
        let registry = HealthRegistry::new();
        assert_eq!(registry.liveness(), HealthState::Healthy);
        assert_eq!(registry.startup(), HealthState::Starting);
        registry.set_startup_complete();
        assert_eq!(registry.startup(), HealthState::Healthy);
        registry.set_startup_failed("failure");
        assert_eq!(registry.startup(), HealthState::Unavailable);
        assert!(registry.shutdown(Duration::from_secs(1)));
    }

    #[test]
    fn stale_threshold_is_configurable_and_rejected_when_zero() {
        let registry = HealthRegistry::new();
        assert!(registry.set_stale_after(Duration::ZERO).is_err());
        registry.set_stale_after(Duration::from_millis(1)).unwrap();
        registry
            .register_check(
                "probe",
                HealthCategory::Custom,
                Duration::from_millis(50),
                true,
                || Ok(()),
            )
            .unwrap();
        registry.refresh().unwrap();
        std::thread::sleep(Duration::from_millis(10));
        assert!(registry.json("readyz").contains("\"stale\":true"));
        assert!(registry.shutdown(Duration::from_secs(1)));
    }
}
