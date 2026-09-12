//! Server-side session management for `std.api.session`.
//!
//! Session identifiers are opaque, cryptographically random values.  The
//! session payload is kept on the configured backend; the language-visible
//! `Session` handle is only a short-lived view of one lookup.  This keeps the
//! cookie/token boundary separate from server-side state and makes revocation
//! effective even when an old language handle is still alive.

use crate::db;
use crate::handles::ApiHandleTable;
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{json, Value};
use spectra_db::redis::{RedisConnection, RedisValue};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const SESSION_ID_BYTES: usize = 32;
const SESSION_ID_HEX_LEN: usize = SESSION_ID_BYTES * 2;
const MAX_SESSION_VALUE_BYTES: usize = 1024 * 1024;
const MAX_REDIS_PREFIX_BYTES: usize = 128;
const MAX_SESSION_KEY_BYTES: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub value: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub max_expires_at_ms: i64,
    pub ttl_ms: i64,
    pub sliding: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    InvalidConfiguration(String),
    InvalidId,
    InvalidValue,
    Randomness,
    NotFound,
    CorruptRecord,
    Backend(String),
}

impl SessionError {
    pub fn code(&self) -> SpectraHostValue {
        match self {
            Self::InvalidConfiguration(_) => 1,
            Self::InvalidId => 2,
            Self::InvalidValue => 3,
            Self::Randomness => 4,
            Self::NotFound => 5,
            Self::CorruptRecord => 6,
            Self::Backend(_) => 7,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidConfiguration(message) => message.clone(),
            Self::InvalidId => "invalid session identifier".to_string(),
            Self::InvalidValue => "session value is empty or exceeds 1 MiB".to_string(),
            Self::Randomness => "secure random source is unavailable".to_string(),
            Self::NotFound => "session was not found or has expired".to_string(),
            Self::CorruptRecord => "session backend returned a corrupt record".to_string(),
            Self::Backend(message) => message.clone(),
        }
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for SessionError {}

type SessionResult<T> = Result<T, SessionError>;

trait SessionBackend: Send + Sync {
    fn kind(&self) -> &'static str;
    fn create(
        &self,
        value: &str,
        now_ms: i64,
        ttl_ms: i64,
        max_lifetime_ms: i64,
        sliding: bool,
    ) -> SessionResult<SessionRecord>;
    fn lookup(&self, id: &str, now_ms: i64, touch: bool) -> SessionResult<Option<SessionRecord>>;
    fn revoke(&self, id: &str) -> SessionResult<bool>;
}

#[derive(Clone)]
pub struct SessionStore {
    backend: Arc<dyn SessionBackend>,
}

impl fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionStore")
            .field("kind", &self.kind())
            .finish()
    }
}

impl SessionStore {
    pub fn memory() -> Self {
        Self {
            backend: Arc::new(MemorySessionBackend::default()),
        }
    }

    pub fn redis(connection: RedisConnection, prefix: impl Into<String>) -> SessionResult<Self> {
        let prefix = prefix.into();
        validate_prefix(&prefix)?;
        Ok(Self {
            backend: Arc::new(RedisSessionBackend { connection, prefix }),
        })
    }

    fn kind(&self) -> &'static str {
        self.backend.kind()
    }

    fn create(
        &self,
        value: &str,
        now_ms: i64,
        ttl_ms: i64,
        max_lifetime_ms: i64,
        sliding: bool,
    ) -> SessionResult<SessionRecord> {
        validate_create_arguments(value, ttl_ms, max_lifetime_ms)?;
        self.backend
            .create(value, now_ms, ttl_ms, max_lifetime_ms, sliding)
    }

    fn lookup(&self, id: &str, now_ms: i64, touch: bool) -> SessionResult<Option<SessionRecord>> {
        validate_id(id)?;
        self.backend.lookup(id, now_ms, touch)
    }

    fn revoke(&self, id: &str) -> SessionResult<bool> {
        validate_id(id)?;
        self.backend.revoke(id)
    }
}

#[derive(Clone)]
pub struct Session {
    backend: Arc<dyn SessionBackend>,
    record: SessionRecord,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.record.id)
            .field("created_at_ms", &self.record.created_at_ms)
            .field("expires_at_ms", &self.record.expires_at_ms)
            .field("sliding", &self.record.sliding)
            .finish()
    }
}

impl Session {
    fn from_record(store: &SessionStore, record: SessionRecord) -> Self {
        Self {
            backend: Arc::clone(&store.backend),
            record,
        }
    }

    fn is_valid(&self, now_ms: i64) -> SessionResult<bool> {
        Ok(self
            .backend
            .lookup(&self.record.id, now_ms, false)?
            .is_some())
    }
}

#[derive(Default)]
struct MemorySessionBackend {
    entries: Mutex<HashMap<String, SessionRecord>>,
}

impl SessionBackend for MemorySessionBackend {
    fn kind(&self) -> &'static str {
        "memory"
    }

    fn create(
        &self,
        value: &str,
        now_ms: i64,
        ttl_ms: i64,
        max_lifetime_ms: i64,
        sliding: bool,
    ) -> SessionResult<SessionRecord> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| SessionError::Backend("memory session store lock poisoned".into()))?;
        entries.retain(|_, record| record.expires_at_ms > now_ms);
        for _ in 0..8 {
            let id = generate_id()?;
            if entries.contains_key(&id) {
                continue;
            }
            let record = new_record(id, value, now_ms, ttl_ms, max_lifetime_ms, sliding)?;
            entries.insert(record.id.clone(), record.clone());
            return Ok(record);
        }
        Err(SessionError::Randomness)
    }

    fn lookup(&self, id: &str, now_ms: i64, touch: bool) -> SessionResult<Option<SessionRecord>> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| SessionError::Backend("memory session store lock poisoned".into()))?;
        let Some(mut record) = entries.get(id).cloned() else {
            return Ok(None);
        };
        if record.expires_at_ms <= now_ms {
            entries.remove(id);
            return Ok(None);
        }
        if touch && record.sliding {
            record.expires_at_ms = refreshed_expiry(&record, now_ms)?;
            entries.insert(id.to_string(), record.clone());
        }
        Ok(Some(record))
    }

    fn revoke(&self, id: &str) -> SessionResult<bool> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| SessionError::Backend("memory session store lock poisoned".into()))?;
        Ok(entries.remove(id).is_some())
    }
}

struct RedisSessionBackend {
    connection: RedisConnection,
    prefix: String,
}

impl SessionBackend for RedisSessionBackend {
    fn kind(&self) -> &'static str {
        "redis"
    }

    fn create(
        &self,
        value: &str,
        now_ms: i64,
        ttl_ms: i64,
        max_lifetime_ms: i64,
        sliding: bool,
    ) -> SessionResult<SessionRecord> {
        for _ in 0..8 {
            let id = generate_id()?;
            let key = self.key(&id)?;
            if self.connection.exists_blocking(&key).map_err(redis_error)? {
                continue;
            }
            let record = new_record(id, value, now_ms, ttl_ms, max_lifetime_ms, sliding)?;
            self.write(&key, &record, now_ms)?;
            return Ok(record);
        }
        Err(SessionError::Randomness)
    }

    fn lookup(&self, id: &str, now_ms: i64, touch: bool) -> SessionResult<Option<SessionRecord>> {
        let key = self.key(id)?;
        let Some(value) = self.connection.get_blocking(&key).map_err(redis_error)? else {
            return Ok(None);
        };
        let bytes = value.into_bytes().map_err(redis_error)?;
        let mut record = decode_record(&bytes)?;
        if record.id != id || record.expires_at_ms <= now_ms {
            let _ = self.connection.delete_blocking(&key).map_err(redis_error)?;
            return Ok(None);
        }
        if touch && record.sliding {
            record.expires_at_ms = refreshed_expiry(&record, now_ms)?;
            self.write(&key, &record, now_ms)?;
        }
        Ok(Some(record))
    }

    fn revoke(&self, id: &str) -> SessionResult<bool> {
        let key = self.key(id)?;
        self.connection.delete_blocking(&key).map_err(redis_error)
    }
}

impl RedisSessionBackend {
    fn key(&self, id: &str) -> SessionResult<String> {
        validate_id(id)?;
        let key = format!("{}{}", self.prefix, id);
        if key.len() > MAX_SESSION_KEY_BYTES {
            return Err(SessionError::InvalidConfiguration(
                "session Redis key exceeds 1024 bytes".into(),
            ));
        }
        Ok(key)
    }

    fn write(&self, key: &str, record: &SessionRecord, now_ms: i64) -> SessionResult<()> {
        let value = encode_record(record)?;
        let remaining_ms = record.expires_at_ms.saturating_sub(now_ms).max(1) as u64;
        self.connection
            .set_blocking(
                key,
                RedisValue::Text(value),
                Some(std::time::Duration::from_millis(remaining_ms)),
            )
            .map_err(redis_error)
    }
}

fn redis_error(error: spectra_db::redis::RedisError) -> SessionError {
    SessionError::Backend(format!("{}: {}", error.code, error.message))
}

fn validate_prefix(prefix: &str) -> SessionResult<()> {
    if prefix.is_empty()
        || prefix.len() > MAX_REDIS_PREFIX_BYTES
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-' | b'.'))
    {
        return Err(SessionError::InvalidConfiguration(
            "session Redis prefix must contain only ASCII key-safe characters and be 1..128 bytes"
                .into(),
        ));
    }
    Ok(())
}

fn validate_id(id: &str) -> SessionResult<()> {
    if id.len() != SESSION_ID_HEX_LEN
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(SessionError::InvalidId);
    }
    Ok(())
}

fn validate_create_arguments(value: &str, ttl_ms: i64, max_lifetime_ms: i64) -> SessionResult<()> {
    if value.is_empty() || value.len() > MAX_SESSION_VALUE_BYTES {
        return Err(SessionError::InvalidValue);
    }
    if ttl_ms <= 0 || max_lifetime_ms < ttl_ms {
        return Err(SessionError::InvalidConfiguration(
            "session TTL must be positive and no greater than the maximum lifetime".into(),
        ));
    }
    Ok(())
}

fn new_record(
    id: String,
    value: &str,
    now_ms: i64,
    ttl_ms: i64,
    max_lifetime_ms: i64,
    sliding: bool,
) -> SessionResult<SessionRecord> {
    let max_expires_at_ms = now_ms
        .checked_add(max_lifetime_ms)
        .ok_or_else(|| SessionError::InvalidConfiguration("session lifetime overflow".into()))?;
    let expires_at_ms = now_ms
        .checked_add(ttl_ms)
        .ok_or_else(|| SessionError::InvalidConfiguration("session TTL overflow".into()))?
        .min(max_expires_at_ms);
    Ok(SessionRecord {
        id,
        value: value.to_string(),
        created_at_ms: now_ms,
        expires_at_ms,
        max_expires_at_ms,
        ttl_ms,
        sliding,
    })
}

fn refreshed_expiry(record: &SessionRecord, now_ms: i64) -> SessionResult<i64> {
    now_ms
        .checked_add(record.ttl_ms)
        .map(|candidate| candidate.min(record.max_expires_at_ms))
        .ok_or_else(|| SessionError::InvalidConfiguration("session TTL overflow".into()))
}

fn generate_id() -> SessionResult<String> {
    let mut bytes = [0_u8; SESSION_ID_BYTES];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| SessionError::Randomness)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn encode_record(record: &SessionRecord) -> SessionResult<String> {
    serde_json::to_string(&json!({
        "id": record.id,
        "value": record.value,
        "created_at_ms": record.created_at_ms,
        "expires_at_ms": record.expires_at_ms,
        "max_expires_at_ms": record.max_expires_at_ms,
        "ttl_ms": record.ttl_ms,
        "sliding": record.sliding,
    }))
    .map_err(|_| SessionError::CorruptRecord)
}

fn decode_record(bytes: &[u8]) -> SessionResult<SessionRecord> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| SessionError::CorruptRecord)?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or(SessionError::CorruptRecord)?
        .to_string();
    validate_id(&id)?;
    let session_value = value
        .get("value")
        .and_then(Value::as_str)
        .ok_or(SessionError::CorruptRecord)?
        .to_string();
    let created_at_ms = value
        .get("created_at_ms")
        .and_then(Value::as_i64)
        .ok_or(SessionError::CorruptRecord)?;
    let expires_at_ms = value
        .get("expires_at_ms")
        .and_then(Value::as_i64)
        .ok_or(SessionError::CorruptRecord)?;
    let max_expires_at_ms = value
        .get("max_expires_at_ms")
        .and_then(Value::as_i64)
        .ok_or(SessionError::CorruptRecord)?;
    let ttl_ms = value
        .get("ttl_ms")
        .and_then(Value::as_i64)
        .ok_or(SessionError::CorruptRecord)?;
    let sliding = value
        .get("sliding")
        .and_then(Value::as_bool)
        .ok_or(SessionError::CorruptRecord)?;
    validate_create_arguments(
        &session_value,
        ttl_ms,
        max_expires_at_ms.saturating_sub(created_at_ms),
    )?;
    if expires_at_ms > max_expires_at_ms || expires_at_ms <= created_at_ms {
        return Err(SessionError::CorruptRecord);
    }
    Ok(SessionRecord {
        id,
        value: session_value,
        created_at_ms,
        expires_at_ms,
        max_expires_at_ms,
        ttl_ms,
        sliding,
    })
}

struct SessionStoreState {
    stores: ApiHandleTable<SessionStore>,
    sessions: ApiHandleTable<Session>,
    last_error: Option<SessionError>,
}

impl SessionStoreState {
    fn new() -> Self {
        Self {
            stores: ApiHandleTable::new(HandleKind::ApiSessionStore),
            sessions: ApiHandleTable::new(HandleKind::ApiSession),
            last_error: None,
        }
    }
}

fn state() -> &'static Mutex<SessionStoreState> {
    static STATE: OnceLock<Mutex<SessionStoreState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(SessionStoreState::new()))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn clear_error() {
    state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .last_error = None;
}

fn fail(ctx: *mut SpectraHostCallContext, error: SessionError) -> i32 {
    let mut state = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.last_error = Some(error);
    write_result(ctx, 0)
}

fn read_store(handle: SpectraHostValue) -> Option<SessionStore> {
    state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .stores
        .get(&handle)
        .cloned()
}

fn read_session(handle: SpectraHostValue) -> Option<Session> {
    state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .sessions
        .get(&handle)
        .cloned()
}

pub extern "C" fn memory_store(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if !args.is_empty() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    clear_error();
    let mut state = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, state.stores.insert(SessionStore::memory()))
}

pub extern "C" fn redis_store(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(prefix) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let connection = match db::redis_connection(args[0]) {
        Ok(connection) => connection,
        Err(error) => return fail(ctx, SessionError::Backend(error.to_string())),
    };
    let store = match SessionStore::redis(connection, prefix) {
        Ok(store) => store,
        Err(error) => return fail(ctx, error),
    };
    clear_error();
    let mut state = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, state.stores.insert(store))
}

pub extern "C" fn store_kind(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(store) = read_store(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    clear_error();
    write_result(ctx, alloc_spectra_string(store.kind()))
}

pub extern "C" fn create(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(store) = read_store(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(value) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let record = match store.create(&value, now_ms(), args[2], args[3], args[4] != 0) {
        Ok(record) => record,
        Err(error) => return fail(ctx, error),
    };
    clear_error();
    let session = Session::from_record(&store, record);
    let mut state = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, state.sessions.insert(session))
}

pub extern "C" fn lookup(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(store) = read_store(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(id) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let record = match store.lookup(&id, now_ms(), true) {
        Ok(Some(record)) => record,
        Ok(None) => return fail(ctx, SessionError::NotFound),
        Err(error) => return fail(ctx, error),
    };
    clear_error();
    let session = Session::from_record(&store, record);
    let mut state = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, state.sessions.insert(session))
}

pub extern "C" fn id(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(session) = read_session(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    clear_error();
    write_result(ctx, alloc_spectra_string(&session.record.id))
}

pub extern "C" fn value(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(session) = read_session(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    clear_error();
    write_result(ctx, alloc_spectra_string(&session.record.value))
}

pub extern "C" fn created_at_ms(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(session) = read_session(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    clear_error();
    write_result(ctx, session.record.created_at_ms)
}

pub extern "C" fn expires_at_ms(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(session) = read_session(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    clear_error();
    write_result(ctx, session.record.expires_at_ms)
}

pub extern "C" fn is_valid(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(session) = read_session(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match session.is_valid(now_ms()) {
        Ok(valid) => {
            clear_error();
            write_result(ctx, valid as SpectraHostValue)
        }
        Err(error) => fail(ctx, error),
    }
}

pub extern "C" fn revoke(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(store) = read_store(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(id) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match store.revoke(&id) {
        Ok(revoked) => {
            clear_error();
            write_result(ctx, revoked as SpectraHostValue)
        }
        Err(error) => fail(ctx, error),
    }
}

pub extern "C" fn error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if !args.is_empty() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let code = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .last_error
        .as_ref()
        .map(SessionError::code)
        .unwrap_or(0);
    write_result(ctx, code)
}

pub extern "C" fn error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if !args.is_empty() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let message = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .last_error
        .as_ref()
        .map(SessionError::message)
        .unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_creates_looks_up_and_expires_sessions() {
        let backend = MemorySessionBackend::default();
        let record = backend
            .create("user-42", 1_000, 100, 500, false)
            .expect("create");
        assert_eq!(record.value, "user-42");
        assert_eq!(
            backend.lookup(&record.id, 1_099, true).unwrap(),
            Some(record.clone())
        );
        assert_eq!(backend.lookup(&record.id, 1_100, true).unwrap(), None);
        assert!(!backend.revoke(&record.id).unwrap());
    }

    #[test]
    fn sliding_expiration_extends_activity_but_respects_maximum_lifetime() {
        let backend = MemorySessionBackend::default();
        let record = backend
            .create("user-42", 1_000, 100, 250, true)
            .expect("create");
        let refreshed = backend
            .lookup(&record.id, 1_080, true)
            .unwrap()
            .expect("active session");
        assert_eq!(refreshed.expires_at_ms, 1_180);
        let capped = backend
            .lookup(&record.id, 1_170, true)
            .unwrap()
            .expect("active session");
        assert_eq!(capped.expires_at_ms, 1_250);
        assert_eq!(backend.lookup(&record.id, 1_250, false).unwrap(), None);
    }

    #[test]
    fn revoke_invalidates_an_active_session_immediately() {
        let backend = MemorySessionBackend::default();
        let record = backend
            .create("user-42", 1_000, 10_000, 20_000, true)
            .expect("create");
        assert!(backend.revoke(&record.id).unwrap());
        assert_eq!(backend.lookup(&record.id, 1_001, false).unwrap(), None);
    }

    #[test]
    fn redis_records_round_trip_without_starting_a_service() {
        let record = SessionRecord {
            id: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            value: "{\"role\":\"admin\"}".into(),
            created_at_ms: 1_000,
            expires_at_ms: 1_100,
            max_expires_at_ms: 2_000,
            ttl_ms: 100,
            sliding: true,
        };
        let encoded = encode_record(&record).expect("encode");
        assert_eq!(decode_record(encoded.as_bytes()).unwrap(), record);
    }

    #[test]
    fn invalid_session_configuration_is_rejected() {
        assert!(validate_create_arguments("value", 0, 10).is_err());
        assert!(validate_create_arguments("value", 20, 10).is_err());
        assert!(validate_prefix("unsafe prefix").is_err());
        assert!(validate_id("not-a-session-id").is_err());
    }
}
