//! Host-call adapter for the HTTP/3 transport.
//!
//! This file is registered as a sibling host adapter by `lib.rs`. The registration table
//! is owned by the integration layer so this adapter can be tested with Rust-owned TLS and
//! handler values without pretending that certificates or callbacks are safely representable
//! by the Spectra string ABI.

use bytes::Bytes;
use crate::http3::{
    Http3Client, Http3ClientConfig, Http3ClientRequest, Http3Error, Http3Handler, Http3Header,
    Http3Request, Http3Response, Http3Server, Http3ServerConfig,
};
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INTERNAL_ERROR,
    HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_NOT_FOUND,
};
use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::runtime::{Builder as RuntimeBuilder, Handle, Runtime};
use tokio::sync::{watch, Mutex as AsyncMutex};

const MAX_HANDLE_SLOTS: usize = 4096;
const HANDLE_SLOT_BITS: u64 = 28;
const HANDLE_GENERATION_BITS: u64 = 28;
const HANDLE_SLOT_MASK: u64 = (1 << HANDLE_SLOT_BITS) - 1;
const HANDLE_GENERATION_MASK: u64 = (1 << HANDLE_GENERATION_BITS) - 1;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_TARGET_BYTES: usize = 8 * 1024;
const MAX_PUBLIC_STRING_BYTES: usize = 1024 * 1024;
const CANCEL_POLL: Duration = Duration::from_millis(10);

const TAG_CONFIG: u8 = 1;
const TAG_HANDLER: u8 = 2;
const TAG_SERVER: u8 = 3;
const TAG_CLIENT: u8 = 4;
const TAG_REQUEST: u8 = 5;
const TAG_RESPONSE: u8 = 6;
const TAG_OUTCOME: u8 = 7;

#[derive(Debug)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

#[derive(Debug)]
struct TypedSlots<T> {
    tag: u8,
    slots: Vec<Slot<T>>,
}

impl<T> TypedSlots<T> {
    fn new(tag: u8) -> Self {
        Self { tag, slots: Vec::new() }
    }

    fn encode(&self, slot: usize, generation: u32) -> Option<SpectraHostValue> {
        let slot = u64::try_from(slot).ok()?.checked_add(1)?;
        if slot > HANDLE_SLOT_MASK || u64::from(generation) > HANDLE_GENERATION_MASK {
            return None;
        }
        let raw = (u64::from(self.tag) << 56)
            | (u64::from(generation) << HANDLE_SLOT_BITS)
            | slot;
        i64::try_from(raw).ok()
    }

    fn decode(&self, raw: SpectraHostValue) -> Option<(usize, u32)> {
        if raw <= 0 {
            return None;
        }
        let raw = u64::try_from(raw).ok()?;
        if (raw >> 56) as u8 != self.tag {
            return None;
        }
        let slot = (raw & HANDLE_SLOT_MASK).checked_sub(1)? as usize;
        let generation = ((raw >> HANDLE_SLOT_BITS) & HANDLE_GENERATION_MASK) as u32;
        Some((slot, generation))
    }

    fn insert(&mut self, value: T) -> Option<SpectraHostValue> {
        if let Some(index) = self.slots.iter().position(|slot| slot.value.is_none()) {
            let generation = self.slots[index].generation;
            self.slots[index].value = Some(value);
            return self.encode(index, generation);
        }
        if self.slots.len() >= MAX_HANDLE_SLOTS {
            return None;
        }
        let index = self.slots.len();
        self.slots.push(Slot { generation: 1, value: Some(value) });
        self.encode(index, 1)
    }

    fn get(&self, raw: SpectraHostValue) -> Option<&T> {
        let (slot, generation) = self.decode(raw)?;
        let slot = self.slots.get(slot)?;
        if slot.generation != generation {
            return None;
        }
        slot.value.as_ref()
    }


    fn remove(&mut self, raw: SpectraHostValue) -> Option<T> {
        let (slot, generation) = self.decode(raw)?;
        let slot = self.slots.get_mut(slot)?;
        if slot.generation != generation { return None; }
        let old = slot.value.take();
        if old.is_some() {
            slot.generation = (slot.generation.wrapping_add(1) & HANDLE_GENERATION_MASK as u32).max(1);
        }
        old
    }
}

#[derive(Clone)]
enum ConfigEntry {
    Server(Http3ServerConfig),
    Client(Http3ClientConfig),
}

type HandlerEntry = Http3Handler;
type ServerEntry = Arc<AsyncMutex<Http3Server>>;
type ClientEntry = Arc<Http3Client>;
type ResponseEntry = Http3Response;
type OutcomeEntry = Result<SpectraHostValue, Http3Error>;

enum RequestState {
    Draft { client: ClientEntry, request: Http3Request },
    Open {
        stream: Arc<AsyncMutex<Http3ClientRequest>>,
        cancel: watch::Sender<bool>,
        trailers_sent: Arc<AtomicBool>,
        finished: Arc<AtomicBool>,
        response_received: Arc<AtomicBool>,
    },
}

struct RequestEntry {
    state: Mutex<RequestState>,
}

struct Http3HostStore {
    configs: TypedSlots<ConfigEntry>,
    handlers: TypedSlots<HandlerEntry>,
    servers: TypedSlots<ServerEntry>,
    clients: TypedSlots<ClientEntry>,
    requests: TypedSlots<Arc<RequestEntry>>,
    responses: TypedSlots<ResponseEntry>,
    outcomes: TypedSlots<OutcomeEntry>,
}

impl Http3HostStore {
    fn new() -> Self {
        Self {
            configs: TypedSlots::new(TAG_CONFIG),
            handlers: TypedSlots::new(TAG_HANDLER),
            servers: TypedSlots::new(TAG_SERVER),
            clients: TypedSlots::new(TAG_CLIENT),
            requests: TypedSlots::new(TAG_REQUEST),
            responses: TypedSlots::new(TAG_RESPONSE),
            outcomes: TypedSlots::new(TAG_OUTCOME),
        }
    }
}

fn store() -> &'static Mutex<Http3HostStore> {
    static STORE: LazyLock<Mutex<Http3HostStore>> = LazyLock::new(|| Mutex::new(Http3HostStore::new()));
    &STORE
}

fn lock_store() -> std::sync::MutexGuard<'static, Http3HostStore> {
    store().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn store_error(message: impl Into<String>) -> Http3Error {
    Http3Error::Runtime(message.into())
}


fn decode_base64(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || value.len() % 4 != 0 {
        return None;
    }
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    for (chunk_index, chunk) in value.as_bytes().chunks_exact(4).enumerate() {
        let mut number = 0u32;
        let mut padding = 0usize;
        for (position, byte) in chunk.iter().copied().enumerate() {
            let digit = match byte {
                b'A'..=b'Z' => Some(byte - b'A'),
                b'a'..=b'z' => Some(byte - b'a' + 26),
                b'0'..=b'9' => Some(byte - b'0' + 52),
                b'+' => Some(62),
                b'/' => Some(63),
                b'=' if position >= 2 => {
                    padding += 1;
                    None
                }
                _ => return None,
            };
            if let Some(digit) = digit {
                if padding != 0 {
                    return None;
                }
                number = (number << 6) | u32::from(digit);
            } else {
                number <<= 6;
            }
        }
        if padding > 2 || (padding != 0 && chunk_index + 1 != value.len() / 4) {
            return None;
        }
        if padding == 1 && chunk[3] != b'=' {
            return None;
        }
        if padding == 2 && !(chunk[2] == b'=' && chunk[3] == b'=') {
            return None;
        }
        output.push((number >> 16) as u8);
        if padding < 2 {
            output.push((number >> 8) as u8);
        }
        if padding == 0 {
            output.push(number as u8);
        }
    }
    Some(output)
}

/// Decodes one base64 DER blob or a comma-separated list of them. A single
/// entry behaves exactly as before; empty entries fail fast so a trailing
/// comma cannot silently arm a partial chain.
fn decode_base64_list(encoded: &str) -> Option<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    for part in encoded.split(',') {
        if part.is_empty() {
            return None;
        }
        out.push(decode_base64(part)?);
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

pub extern "C" fn http3_server_config_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(address) = read_bounded_string(args[0], 256).and_then(|value| value.parse::<SocketAddr>().ok()) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(chain), Some(key)) = (
        read_bounded_string(args[1], MAX_PUBLIC_STRING_BYTES).and_then(|value| decode_base64_list(&value)),
        read_bounded_string(args[2], MAX_PUBLIC_STRING_BYTES).and_then(|value| decode_base64(&value)),
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(tls) = crate::tls::server_config_from_der(chain, key, vec![b"h3".to_vec()]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(handle) = lock_store().configs.insert(ConfigEntry::Server(
        Http3ServerConfig::from_tls(address, tls),
    )) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    write_result(ctx, handle)
}

pub extern "C" fn http3_client_config_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(encoded) = read_bounded_string(args[0], MAX_PUBLIC_STRING_BYTES) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut roots = rustls::RootCertStore::empty();
    if !encoded.is_empty() {
        let Some(chain) = decode_base64_list(&encoded) else { return HOST_STATUS_INVALID_ARGUMENT; };
        for cert in chain {
            if roots.add(rustls::pki_types::CertificateDer::from(cert)).is_err() {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
        }
    }
    let config = Http3ClientConfig::default().with_root_certificates(Arc::new(roots));
    let Some(handle) = lock_store().configs.insert(ConfigEntry::Client(config)) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    write_result(ctx, handle)
}

pub extern "C" fn http3_handler_text(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Ok(status) = u16::try_from(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    if !(100..=999).contains(&status) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let Some(body) = read_bounded_string(args[1], MAX_PUBLIC_STRING_BYTES) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let handler: Http3Handler = Arc::new(move |_| {
        let body = body.clone();
        Box::pin(async move { Ok(Http3Response::text(status, body)) })
    });
    let Some(handle) = lock_store().handlers.insert(handler) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    write_result(ctx, handle)
}

fn insert_outcome(outcome: OutcomeEntry) -> Result<SpectraHostValue, ()> {
    lock_store().outcomes.insert(outcome).ok_or(())
}

async fn cancellable<F, T>(token: Arc<AtomicBool>, future: F) -> Result<T, Http3Error>
where
    F: Future<Output = Result<T, Http3Error>>,
{
    tokio::pin!(future);
    loop {
        if token.load(Ordering::Acquire) { return Err(Http3Error::Cancelled); }
        tokio::select! {
            result = &mut future => return result,
            _ = tokio::time::sleep(CANCEL_POLL) => {},
        }
    }
}

fn background_runtime() -> &'static Runtime {
    static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
        RuntimeBuilder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("HTTP/3 background runtime")
    });
    &RUNTIME
}

fn runtime_handle() -> Handle {
    Handle::try_current().unwrap_or_else(|_| background_runtime().handle().clone())
}

fn spawn_http3_task<F>(future: F) -> Result<SpectraHostValue, i32>
where
    F: Future<Output = Result<SpectraHostValue, Http3Error>> + Send + 'static,
{
    let runtime = runtime_handle();
    spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let outcome = runtime.block_on(cancellable(token, future));
        insert_outcome(outcome).map_err(|_| ())
    })
}


fn read_bounded_string(value: SpectraHostValue, max: usize) -> Option<String> {
    let value = read_spectra_string(value)?;
    (value.len() <= max).then_some(value)
}

fn header_size(name: &str, value: &str) -> usize {
    name.len().saturating_add(value.len()).saturating_add(32)
}

fn valid_header(name: &str, value: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        && value.bytes().all(|byte| byte == b'\t' || byte == b' ' || (0x21..=0x7e).contains(&byte) || byte >= 0x80)
}

pub extern "C" fn http3_server_start(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (config, handler) = {
        let store = lock_store();
        let Some(ConfigEntry::Server(config)) = store.configs.get(args[0]).cloned() else { return HOST_STATUS_INVALID_ARGUMENT; };
        let Some(handler) = store.handlers.get(args[1]).cloned() else { return HOST_STATUS_INVALID_ARGUMENT; };
        (config, handler)
    };
    let runtime = runtime_handle();
    let _guard = runtime.enter();
    let server = match Http3Server::start(config, handler) {
        Ok(server) => Arc::new(AsyncMutex::new(server)),
        Err(_) => return HOST_STATUS_INVALID_ARGUMENT,
    };
    let Some(handle) = lock_store().servers.insert(server) else { return HOST_STATUS_INTERNAL_ERROR; };
    write_result(ctx, handle)
}

pub extern "C" fn http3_server_local_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(server) = store.servers.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Ok(server) = server.try_lock() else { return HOST_STATUS_INVALID_ARGUMENT; };
    write_result(ctx, i64::from(server.local_addr().port()))
}

pub extern "C" fn http3_server_shutdown(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let server = {
        let store = lock_store();
        let Some(server) = store.servers.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        Arc::clone(server)
    };
    let task = spawn_http3_task(async move {
        let mut server = server.lock().await;
        server.shutdown().await.map(|_| 1).map_err(|error| error)
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

pub extern "C" fn http3_client_connect(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(url) = read_bounded_string(args[1], MAX_PUBLIC_STRING_BYTES) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let config = {
        let store = lock_store();
        let Some(ConfigEntry::Client(config)) = store.configs.get(args[0]).cloned() else { return HOST_STATUS_INVALID_ARGUMENT; };
        config
    };
    let task = spawn_http3_task(async move {
        let client = Http3Client::connect(&url, config).await?;
        let handle = lock_store().clients.insert(Arc::new(client)).ok_or_else(|| store_error("HTTP/3 client table is full"))?;
        Ok(handle)
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

pub extern "C" fn http3_client_shutdown(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let client = {
        let store = lock_store();
        let Some(client) = store.clients.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        Arc::clone(client)
    };
    let task = spawn_http3_task(async move {
        client.shutdown().await.map(|_| 1)
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

pub extern "C" fn http3_client_request_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(method) = read_bounded_string(args[1], 128) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(target) = read_bounded_string(args[2], MAX_TARGET_BYTES) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let client = {
        let store = lock_store();
        let Some(client) = store.clients.get(args[0]).cloned() else { return HOST_STATUS_INVALID_ARGUMENT; };
        client
    };
    let entry = Arc::new(RequestEntry { state: Mutex::new(RequestState::Draft {
        client,
        request: Http3Request { method, target, headers: Vec::new(), body: Vec::new(), trailers: Vec::new() },
    }) });

    let Some(handle) = lock_store().requests.insert(entry) else { return HOST_STATUS_INTERNAL_ERROR; };
    write_result(ctx, handle)
}

pub extern "C" fn http3_client_request_header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(name) = read_bounded_string(args[1], MAX_HEADER_BYTES) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(value) = read_bounded_string(args[2], MAX_HEADER_BYTES) else { return HOST_STATUS_INVALID_ARGUMENT; };
    if !valid_header(&name, &value) || header_size(&name, &value) > MAX_HEADER_BYTES { return HOST_STATUS_INVALID_ARGUMENT; }
    let store = lock_store();
    let Some(entry) = store.requests.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = entry.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let RequestState::Draft { request, .. } = &mut *state else { return HOST_STATUS_INVALID_ARGUMENT; };
    let total = request.headers.iter().map(|header| header_size(&header.name, &header.value)).sum::<usize>();
    if total.saturating_add(header_size(&name, &value)) > MAX_HEADER_BYTES { return HOST_STATUS_INVALID_ARGUMENT; }
    request.headers.push(Http3Header { name, value });
    write_result(ctx, 1)
}

pub extern "C" fn http3_client_request_open(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let request_handle = args[0];
    let (entry, client, request) = {
        let store = lock_store();
        let Some(entry) = store.requests.get(request_handle).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let state = entry.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (client, request) = match &*state {
            RequestState::Draft { client, request } => (Arc::clone(client), request.clone()),
            _ => return HOST_STATUS_INVALID_ARGUMENT,
        };
        drop(state);
        (entry, client, request)
    };
    let task = spawn_http3_task(async move {
        let stream = client.open_request(request).await?;
        let (cancel, _) = watch::channel(false);
        let stream = Arc::new(AsyncMutex::new(stream));
        let can_install = {
            let mut state = entry.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if !matches!(*state, RequestState::Draft { .. }) {
                false
            } else {
                *state = RequestState::Open {
                    stream: Arc::clone(&stream),
                    cancel,
                    trailers_sent: Arc::new(AtomicBool::new(false)),
                    finished: Arc::new(AtomicBool::new(false)),
                    response_received: Arc::new(AtomicBool::new(false)),
                };
                true
            }
        };
        if !can_install {
            let mut stream = stream.lock().await;
            stream.cancel();
            return Err(Http3Error::Cancelled);
        }
        Ok(request_handle)
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

fn stream_parts(entry: &RequestEntry) -> Result<(Arc<AsyncMutex<Http3ClientRequest>>, watch::Receiver<bool>, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>), Http3Error> {
    let state = entry.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let RequestState::Open { stream, cancel, trailers_sent, finished, response_received } = &*state else {
        return Err(Http3Error::Protocol("HTTP/3 request is not open".into()));
    };
    Ok((Arc::clone(stream), cancel.subscribe(), Arc::clone(trailers_sent), Arc::clone(finished), Arc::clone(response_received)))
}

async fn stream_cancelled(rx: &mut watch::Receiver<bool>) -> bool {
    loop {
        tokio::select! {
            changed = rx.changed() => if changed.is_ok() && *rx.borrow() { return true; },
            _ = tokio::time::sleep(CANCEL_POLL) => {},
        }
    }
}

pub extern "C" fn http3_client_request_send_body(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(body) = read_bounded_string(args[1], MAX_PUBLIC_STRING_BYTES) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let entry = {
        let store = lock_store();
        let Some(entry) = store.requests.get(args[0]).cloned() else { return HOST_STATUS_INVALID_ARGUMENT; };
        entry
    };
    let task = spawn_http3_task(async move {
        let (stream, mut cancel, _, _, _) = stream_parts(&entry)?;
        let operation = async move {
            tokio::select! {
                result = async { stream.lock().await.send_data(Bytes::from(body.into_bytes())).await } => result,
                _ = stream_cancelled(&mut cancel) => Err(Http3Error::Cancelled),
            }
        };
        operation.await.map(|_| 1)
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

pub extern "C" fn http3_client_request_send_trailers(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(name) = read_bounded_string(args[1], MAX_HEADER_BYTES) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(value) = read_bounded_string(args[2], MAX_HEADER_BYTES) else { return HOST_STATUS_INVALID_ARGUMENT; };
    if !valid_header(&name, &value) || header_size(&name, &value) > MAX_HEADER_BYTES { return HOST_STATUS_INVALID_ARGUMENT; }
    let entry = {
        let store = lock_store();
        let Some(entry) = store.requests.get(args[0]).cloned() else { return HOST_STATUS_INVALID_ARGUMENT; };
        entry
    };
    let task = spawn_http3_task(async move {
        let (stream, mut cancel, trailers_sent, _, _) = stream_parts(&entry)?;
        if trailers_sent.swap(true, Ordering::AcqRel) { return Err(Http3Error::Protocol("HTTP/3 trailers already sent".into())); }
        tokio::select! {
            result = async { stream.lock().await.send_trailers(vec![Http3Header { name, value }]).await } => result,
            _ = stream_cancelled(&mut cancel) => Err(Http3Error::Cancelled),
        }
        .map(|_| 1)
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

pub extern "C" fn http3_client_request_finish(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let entry = {
        let store = lock_store();
        let Some(entry) = store.requests.get(args[0]).cloned() else { return HOST_STATUS_INVALID_ARGUMENT; };
        entry
    };
    let task = spawn_http3_task(async move {
        let (stream, mut cancel, _, finished, _) = stream_parts(&entry)?;
        if finished.swap(true, Ordering::AcqRel) { return Err(Http3Error::Protocol("HTTP/3 request already finished".into())); }
        tokio::select! {
            result = async { stream.lock().await.finish().await } => result,
            _ = stream_cancelled(&mut cancel) => Err(Http3Error::Cancelled),
        }
        .map(|_| 1)
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

pub extern "C" fn http3_client_request_receive_response(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let entry = {
        let store = lock_store();
        let Some(entry) = store.requests.get(args[0]).cloned() else { return HOST_STATUS_INVALID_ARGUMENT; };
        entry
    };
    let task = spawn_http3_task(async move {
        let (stream, mut cancel, _, _, response_received) = stream_parts(&entry)?;
        if response_received.swap(true, Ordering::AcqRel) { return Err(Http3Error::Protocol("HTTP/3 response already received".into())); }
        let response = tokio::select! {
            result = async { stream.lock().await.recv_response().await } => result,
            _ = stream_cancelled(&mut cancel) => Err(Http3Error::Cancelled),
        }?;
        let handle = lock_store().responses.insert(response).ok_or_else(|| store_error("HTTP/3 response table is full"))?;
        Ok(handle)
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

pub extern "C" fn http3_client_request_cancel(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(entry) = store.requests.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = entry.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let RequestState::Open { cancel, .. } = &*state else { return HOST_STATUS_INVALID_ARGUMENT; };
    let _ = cancel.send(true);
    write_result(ctx, 1)
}

pub extern "C" fn http3_response_status(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(response) = store.responses.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    write_result(ctx, i64::from(response.status_code))
}

fn response_header_value<'a>(response: &'a Http3Response, name: &str, trailers: bool) -> Option<&'a str> {
    let headers = if trailers { &response.trailers } else { &response.headers };
    headers.iter().find(|header| header.name.eq_ignore_ascii_case(name)).map(|header| header.value.as_str())
}

pub extern "C" fn http3_response_header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(name) = read_bounded_string(args[1], MAX_HEADER_BYTES) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(response) = store.responses.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(value) = response_header_value(response, &name, false) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, alloc_spectra_string(value))
}

pub extern "C" fn http3_response_trailer(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(name) = read_bounded_string(args[1], MAX_HEADER_BYTES) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(response) = store.responses.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(value) = response_header_value(response, &name, true) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, alloc_spectra_string(value))
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (a << 16) | (b << 8) | c;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// Returns response bytes as bounded base64 text, preserving arbitrary binary bodies over the
/// current NUL-terminated Spectra string ABI.
pub extern "C" fn http3_response_body_base64(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(response) = store.responses.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    write_result(ctx, alloc_spectra_string(&base64_encode(&response.body)))
}

pub extern "C" fn http3_response_body_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(response) = store.responses.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Ok(length) = i64::try_from(response.body.len()) else { return HOST_STATUS_INTERNAL_ERROR; };
    write_result(ctx, length)
}

pub extern "C" fn http3_task_result(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    match spectra_runtime::stdlib::task_result_value(args[0]) {
        Ok(value) => write_result(ctx, value),
        Err(status) => status,
    }
}

pub extern "C" fn http3_task_cancel(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    write_result(ctx, i64::from(spectra_runtime::stdlib::cancel_task_handle(args[0])))
}

pub extern "C" fn http3_result_ok(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(result) = store.outcomes.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn http3_result_value(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(Ok(value)) = store.outcomes.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    write_result(ctx, *value)
}

pub extern "C" fn http3_result_error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(Err(error)) = store.outcomes.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    write_result(ctx, i64::from(http3_error_code(error)))
}

pub extern "C" fn http3_result_error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let store = lock_store();
    let Some(Err(error)) = store.outcomes.get(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    write_result(ctx, alloc_spectra_string(&error.to_string()))
}

fn http3_error_code(error: &Http3Error) -> u8 {
    match error {
        Http3Error::Io(_) => 1,
        Http3Error::Tls(_) => 2,
        Http3Error::Protocol(_) => 3,
        Http3Error::InvalidUrl(_) => 4,
        Http3Error::InvalidHeader(_) => 5,
        Http3Error::BodyTooLarge { .. } => 6,
        Http3Error::HeaderTooLarge { .. } => 7,
        Http3Error::Timeout => 8,
        Http3Error::Cancelled => 9,
        Http3Error::Closed => 10,
        Http3Error::AlreadyStopped => 11,
        Http3Error::Runtime(_) => 12,
    }
}

pub extern "C" fn http3_handle_drop(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut store = lock_store();
    let raw = args[0];
    let removed = store.configs.remove(raw).is_some()
        || store.handlers.remove(raw).is_some()
        || store.servers.remove(raw).is_some()
        || store.clients.remove(raw).is_some()
        || store.requests.remove(raw).is_some()
        || store.responses.remove(raw).is_some()
        || store.outcomes.remove(raw).is_some();
    write_result(ctx, i64::from(removed))
}


#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::generate_simple_self_signed;
    use spectra_runtime::ffi::HOST_STATUS_SUCCESS;

    fn call_host(
        function: extern "C" fn(*mut SpectraHostCallContext) -> i32,
        args: &[SpectraHostValue],
    ) -> (i32, SpectraHostValue) {
        let mut result = [0_i64];
        let mut ctx = SpectraHostCallContext {
            args: args.as_ptr(),
            arg_len: args.len(),
            results: result.as_mut_ptr(),
            result_len: result.len(),
            invoke_fn: None,
        };
        let status = function(&mut ctx);
        (status, result[0])
    }

    fn self_signed_b64(name: &str) -> (String, String) {
        let certified =
            generate_simple_self_signed(vec![name.to_string()]).expect("certificate");
        (
            crate::grpc_host::encode_base64(certified.cert.der()),
            crate::grpc_host::encode_base64(&certified.key_pair.serialize_der()),
        )
    }

    #[test]
    fn server_config_accepts_multi_entry_chain_and_rejects_trailing_comma() {
        let (cert, key) = self_signed_b64("localhost");
        // Two entries (same cert twice keeps the key matching) must behave
        // like the proven single-entry path.
        let chain = format!("{cert},{cert}");
        let (status, handle) = call_host(
            http3_server_config_new,
            &[
                alloc_spectra_string("127.0.0.1:0"),
                alloc_spectra_string(&chain),
                alloc_spectra_string(&key),
            ],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert!(handle != 0);
        let trailing = format!("{cert},");
        let (status, _) = call_host(
            http3_server_config_new,
            &[
                alloc_spectra_string("127.0.0.1:0"),
                alloc_spectra_string(&trailing),
                alloc_spectra_string(&key),
            ],
        );
        assert_eq!(status, HOST_STATUS_INVALID_ARGUMENT);
    }

    #[test]
    fn client_config_accepts_multi_entry_roots() {
        let (first, _) = self_signed_b64("one.example");
        let (second, _) = self_signed_b64("two.example");
        let (status, handle) = call_host(
            http3_client_config_new,
            &[alloc_spectra_string(&format!("{first},{second}"))],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert!(handle != 0);
        let (status, _) = call_host(
            http3_client_config_new,
            &[alloc_spectra_string("!!!not-base64!!!")],
        );
        assert_eq!(status, HOST_STATUS_INVALID_ARGUMENT);
    }
}
