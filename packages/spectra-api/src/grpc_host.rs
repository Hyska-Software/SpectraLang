// Host-call adapter for the real gRPC transport in this module.
//
// The adapter deliberately keeps protocol objects behind generational handles. Binary
// protobuf and metadata values cross the ABI only as base64 strings; a host string is
// never treated as an arbitrary protobuf byte buffer. Calls that wait on sockets use
// the runtime's cancellable Task implementation and a per-client Tokio runtime.

use crate::grpc::{
    GrpcBoxFuture, GrpcClient, GrpcClientStream, GrpcCode, GrpcError, GrpcMessage, GrpcMetadata,
    GrpcReceiver, GrpcRequest, GrpcResponse, GrpcServer, GrpcServerConfig, GrpcService,
    GrpcStatus, GrpcTrailers, GrpcTlsIdentity,
};
use crate::handles::ApiHandleTable;
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INTERNAL_ERROR,
    HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_NOT_FOUND,
};
use spectra_runtime::handles::HandleKind;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const MAX_HOST_BYTES: usize = 16 * 1024 * 1024;


type Runtime = tokio::runtime::Runtime;
type CancellationToken = spectra_runtime::stdlib::CancellationToken;

#[derive(Clone, Debug)]
struct GrpcHostError {
    code: SpectraHostValue,
    message: String,
    details: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
struct GrpcHostResponse {
    message: SpectraHostValue,
    trailers: GrpcTrailers,
}

struct ClientEntry {
    runtime: Arc<Runtime>,
    client: Arc<tokio::sync::Mutex<GrpcClient>>,
}

struct ServerEntry {
    server: GrpcServer,
    // The accept loop spawns on this runtime; dropping it would kill a bound
    // server as soon as the bind task completes (the client entries retain
    // theirs for the same reason).
    #[allow(dead_code)]
    runtime: Arc<Runtime>,
}

enum HostStreamInner {
    Client(GrpcClientStream),
    Server(GrpcReceiver),
}

struct StreamEntry {
    runtime: Arc<Runtime>,
    inner: Arc<tokio::sync::Mutex<HostStreamInner>>,
}

struct GrpcHostStore {
    messages: ApiHandleTable<GrpcMessage>,
    clients: ApiHandleTable<ClientEntry>,
    streams: ApiHandleTable<StreamEntry>,
    metadata: ApiHandleTable<GrpcMetadata>,
    statuses: ApiHandleTable<GrpcStatus>,
    responses: ApiHandleTable<GrpcHostResponse>,
    errors: ApiHandleTable<GrpcHostError>,
    servers: ApiHandleTable<ServerEntry>,
    services: ApiHandleTable<Arc<dyn GrpcService>>,
    callback_services: ApiHandleTable<Arc<CallbackGrpcService>>,
}

impl GrpcHostStore {
    fn new() -> Self {
        Self {
            messages: ApiHandleTable::new(HandleKind::ApiGrpcMessage),
            clients: ApiHandleTable::new(HandleKind::ApiGrpcClient),
            streams: ApiHandleTable::new(HandleKind::ApiGrpcStream),
            metadata: ApiHandleTable::new(HandleKind::ApiGrpcMetadata),
            statuses: ApiHandleTable::new(HandleKind::ApiGrpcStatus),
            responses: ApiHandleTable::new(HandleKind::ApiGrpcResponse),
            errors: ApiHandleTable::new(HandleKind::ApiGrpcError),
            servers: ApiHandleTable::new(HandleKind::ApiGrpcServer),
            services: ApiHandleTable::new(HandleKind::ApiGrpcService),
            callback_services: ApiHandleTable::new(HandleKind::ApiGrpcService),
        }
    }
}

fn store() -> &'static Mutex<GrpcHostStore> {
    static STORE: OnceLock<Mutex<GrpcHostStore>> = OnceLock::new();

    STORE.get_or_init(|| Mutex::new(GrpcHostStore::new()))
}

/// Language-backed gRPC service: each method path dispatches to a Spectra
/// closure through the handler callback bridge (the same bridge GraphQL
/// resolvers use). The closure receives one JSON document and returns one:
///
/// input:  `{"path": str, "messages": [base64], "metadata": {name: base64}}`
/// output: `{"messages": [base64], "status": {"code": int, "message": str}, "metadata": {name: base64}}`
///
/// The full inbound stream is collected before the single invocation and the
/// returned messages stream out, so unary, client-streaming, and
/// server-streaming all work; bidirectional streams are buffered rather than
/// interleaved. A sync closure returns the JSON string directly; an async
/// closure's task handle is resolved first. Anything else (unknown path
/// aside, which yields UNIMPLEMENTED) fails with an Internal error instead
/// of inventing a response.
struct CallbackGrpcService {
    methods: Mutex<std::collections::HashMap<String, crate::handler::CallbackEntry>>,
}

impl CallbackGrpcService {
    fn new() -> Self {
        Self {
            methods: Mutex::new(std::collections::HashMap::new()),
        }
    }
}

fn grpc_code_from_u64(code: u64) -> GrpcCode {
    match code {
        0 => GrpcCode::Ok,
        1 => GrpcCode::Cancelled,
        2 => GrpcCode::Unknown,
        3 => GrpcCode::InvalidArgument,
        4 => GrpcCode::DeadlineExceeded,
        5 => GrpcCode::NotFound,
        6 => GrpcCode::AlreadyExists,
        7 => GrpcCode::PermissionDenied,
        8 => GrpcCode::ResourceExhausted,
        9 => GrpcCode::FailedPrecondition,
        10 => GrpcCode::Aborted,
        11 => GrpcCode::OutOfRange,
        12 => GrpcCode::Unimplemented,
        13 => GrpcCode::Internal,
        14 => GrpcCode::Unavailable,
        15 => GrpcCode::DataLoss,
        16 => GrpcCode::Unauthenticated,
        _ => GrpcCode::Unknown,
    }
}

fn grpc_handler_input_json(
    path: &str,
    metadata: &GrpcMetadata,
    messages: &[GrpcMessage],
) -> String {
    let metadata_json = metadata
        .iter()
        .map(|entry| {
            format!(
                "{}:{}",
                json_escape(&entry.key),
                json_escape(&encode_base64(&entry.value))
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let messages_json = messages
        .iter()
        .map(|message| format!("\"{}\"", encode_base64(message.as_bytes())))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"path\":{},\"messages\":[{}],\"metadata\":{{{}}}}}",
        json_escape(path),
        messages_json,
        metadata_json
    )
}

fn json_escape(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

fn grpc_handler_output(
    text: &str,
) -> Result<(Vec<GrpcMessage>, GrpcStatus, GrpcMetadata), GrpcError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| GrpcError::Protocol(error.to_string()))?;
    let mut messages = Vec::new();
    if let Some(items) = value.get("messages").and_then(|items| items.as_array()) {
        for item in items {
            let encoded = item.as_str().ok_or_else(|| {
                GrpcError::Protocol("gRPC handler messages must be base64 strings".to_string())
            })?;
            let bytes =
                decode_base64(encoded).map_err(|()| GrpcError::Protocol("gRPC handler message is not valid base64".to_string()))?;
            messages.push(GrpcMessage::from_bytes(bytes));
        }
    }
    let status = match value.get("status") {
        None => GrpcStatus::ok(),
        Some(status) => {
            let code = status
                .get("code")
                .and_then(|code| code.as_u64())
                .map(grpc_code_from_u64)
                .unwrap_or(GrpcCode::Unknown);
            let message = status
                .get("message")
                .and_then(|message| message.as_str())
                .unwrap_or_default()
                .to_string();
            GrpcStatus::new(code, message)
        }
    };
    let mut metadata = GrpcMetadata::new();
    if let Some(entries) = value.get("metadata").and_then(|entries| entries.as_object()) {
        for (key, encoded) in entries {
            let value_str = encoded.as_str().ok_or_else(|| {
                GrpcError::Protocol("gRPC handler metadata values must be base64 strings".to_string())
            })?;
            let bytes =
                decode_base64(value_str).map_err(|()| GrpcError::Protocol("gRPC handler metadata value is not valid base64".to_string()))?;
            metadata
                .append(key.clone(), bytes)
                .map_err(|error| GrpcError::Protocol(error.to_string()))?;
        }
    }
    Ok((messages, status, metadata))
}

fn grpc_callback_result_text(entry: crate::handler::CallbackEntry, arg: SpectraHostValue) -> Option<String> {
    let result =
        crate::handler::invoke_callback(entry, arg).ok()?;
    if let Some(text) = read_spectra_string(result) {
        return Some(text);
    }
    // Async closures resolve to a task handle; settle it before reading.
    let settled = spectra_runtime::stdlib::block_on_task_value(result).ok()?;
    read_spectra_string(settled)
}

impl GrpcService for CallbackGrpcService {
    fn call(&self, request: GrpcRequest) -> GrpcBoxFuture {
        let entry = self
            .methods
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&request.path)
            .cloned();
        Box::pin(async move {
            let Some(entry) = entry else {
                let (sender, response) = GrpcResponse::streaming(1);
                drop(sender);
                return Ok(response.with_status(GrpcStatus::new(
                    GrpcCode::Unimplemented,
                    format!("no handler registered for '{}'", request.path),
                )));
            };
            let mut messages = Vec::new();
            let mut inbound = request.inbound;
            while let Some(item) = inbound.recv().await {
                let message = item.map_err(|error| {
                    GrpcError::Protocol(format!("gRPC handler inbound failed: {error}"))
                })?;
                messages.push(message);
            }
            let payload = grpc_handler_input_json(&request.path, &request.metadata, &messages);
            let arg = alloc_spectra_string(&payload);
            let text = grpc_callback_result_text(entry, arg).ok_or_else(|| {
                GrpcError::Protocol("gRPC handler callback failed".to_string())
            })?;
            let (outbound, status, metadata) = grpc_handler_output(&text)?;
            let (sender, response) = GrpcResponse::streaming(outbound.len().max(1));
            let response = response.with_status(status).with_metadata(metadata);
            for message in outbound {
                let _ = sender.send(message).await;
            }
            Ok(response)
        })
    }
}

fn runtime() -> Result<Arc<Runtime>, i32> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map(Arc::new)
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
}

pub(crate) fn decode_base64(value: &str) -> Result<Vec<u8>, ()> {
    let bytes = value.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(());
    }
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
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
                _ => return Err(()),
            };
            if let Some(digit) = digit {
                if padding != 0 {
                    return Err(());
                }
                number = (number << 6) | u32::from(digit);
            } else {
                number <<= 6;
            }
        }
        if padding > 2 || (padding != 0 && index + 1 != bytes.len() / 4) {
            return Err(());
        }
        if padding == 1 && chunk[3] != b'=' {
            return Err(());
        }
        if padding == 2 && !(chunk[2] == b'=' && chunk[3] == b'=') {
            return Err(());
        }
        output.push((number >> 16) as u8);
        if padding < 2 {
            output.push((number >> 8) as u8);
        }
        if padding == 0 {
            output.push(number as u8);
        }
    }
    if output.len() > MAX_HOST_BYTES {
        return Err(());
    }
    Ok(output)
}

pub(crate) fn encode_base64(value: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(value.len().div_ceil(3) * 4);
    for chunk in value.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let number = (first << 16) | (second << 8) | third;
        output.push(TABLE[((number >> 18) & 63) as usize] as char);
        output.push(TABLE[((number >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((number >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(number & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn grpc_code_value(code: GrpcCode) -> SpectraHostValue {
    code as SpectraHostValue
}

fn grpc_code(value: SpectraHostValue) -> Option<GrpcCode> {
    Some(match value {
        0 => GrpcCode::Ok,
        1 => GrpcCode::Cancelled,
        2 => GrpcCode::Unknown,
        3 => GrpcCode::InvalidArgument,
        4 => GrpcCode::DeadlineExceeded,
        5 => GrpcCode::NotFound,
        6 => GrpcCode::AlreadyExists,
        7 => GrpcCode::PermissionDenied,
        8 => GrpcCode::ResourceExhausted,
        9 => GrpcCode::FailedPrecondition,
        10 => GrpcCode::Aborted,
        11 => GrpcCode::OutOfRange,
        12 => GrpcCode::Unimplemented,
        13 => GrpcCode::Internal,
        14 => GrpcCode::Unavailable,
        15 => GrpcCode::DataLoss,
        16 => GrpcCode::Unauthenticated,
        _ => return None,
    })
}

fn error_code(error: &GrpcError) -> GrpcCode {
    match error {
        GrpcError::Cancelled => GrpcCode::Cancelled,
        GrpcError::DeadlineExceeded => GrpcCode::DeadlineExceeded,
        GrpcError::InvalidPath(_) | GrpcError::InvalidMetadata(_) => GrpcCode::InvalidArgument,
        GrpcError::MissingStatus | GrpcError::InvalidStatus | GrpcError::InvalidStatusMessage => {
            GrpcCode::Internal
        }
        _ => GrpcCode::Internal,
    }
}

fn error_handle(error: GrpcError) -> SpectraHostValue {
    let record = GrpcHostError {
        code: grpc_code_value(error_code(&error)),
        message: error.to_string(),
        details: None,
    };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state.errors.insert(record)
}

fn metadata_handle(raw: SpectraHostValue) -> Option<GrpcMetadata> {
    if raw == 0 {
        return Some(GrpcMetadata::new());
    }
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state.metadata.get(&raw).cloned()
}

fn timeout_from_ms(value: SpectraHostValue) -> Option<Duration> {
    if value < 0 {
        None
    } else {
        Some(Duration::from_millis(value as u64))
    }
}

async fn with_cancellation<F, T>(future: F, token: CancellationToken) -> Result<T, GrpcError>
where
    F: Future<Output = Result<T, GrpcError>>,
{
    tokio::pin!(future);
    loop {
        if token.load(std::sync::atomic::Ordering::Acquire) {
            return Err(GrpcError::Cancelled);
        }
        tokio::select! {
            result = &mut future => return result,
            _ = tokio::time::sleep(Duration::from_millis(10)) => {},
        }
    }
}

fn spawn_grpc_task<F, T, Convert>(runtime: Arc<Runtime>, future: F, convert: Convert) -> Result<SpectraHostValue, i32>
where
    F: Future<Output = Result<T, GrpcError>> + Send + 'static,
    T: Send + 'static,
    Convert: FnOnce(T) -> Result<SpectraHostValue, GrpcError> + Send + 'static,
{
    spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let value = runtime.block_on(with_cancellation(future, token));
        let result = match value {
            Ok(value) => convert(value).unwrap_or_else(error_handle),
            Err(error) => error_handle(error),
        };
        Ok(result)
    })
}


pub extern "C" fn grpc_message_from_base64(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(encoded) = read_spectra_string(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Ok(bytes) = decode_base64(&encoded) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, state.messages.insert(GrpcMessage::from_bytes(bytes)))
}

pub extern "C" fn grpc_message_to_base64(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(message) = state.messages.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, alloc_spectra_string(&encode_base64(message.as_bytes())))
}

pub extern "C" fn grpc_message_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(message) = state.messages.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, message.len() as SpectraHostValue)
}

pub extern "C" fn grpc_message_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.messages.remove(&args[0]).is_none() { return HOST_STATUS_NOT_FOUND; }
    write_result(ctx, 1)
}

pub extern "C" fn grpc_metadata_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(_args) = read_args(ctx, 0) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, state.metadata.insert(GrpcMetadata::new()))
}

pub extern "C" fn grpc_metadata_insert(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(key) = read_spectra_string(args[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(encoded) = read_spectra_string(args[2]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Ok(value) = decode_base64(&encoded) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(metadata) = state.metadata.get_mut(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    if metadata.insert(key, value).is_err() { return HOST_STATUS_INVALID_ARGUMENT; }
    write_result(ctx, 1)
}

pub extern "C" fn grpc_metadata_append(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(key) = read_spectra_string(args[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(encoded) = read_spectra_string(args[2]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Ok(value) = decode_base64(&encoded) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(metadata) = state.metadata.get_mut(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    if metadata.append(key, value).is_err() { return HOST_STATUS_INVALID_ARGUMENT; }
    write_result(ctx, 1)
}

pub extern "C" fn grpc_metadata_get(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(key) = read_spectra_string(args[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(metadata) = state.metadata.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, alloc_spectra_string(&encode_base64(metadata.get(&key).unwrap_or(&[]))))
}

pub extern "C" fn grpc_metadata_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(metadata) = state.metadata.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, metadata.iter().count() as SpectraHostValue)
}

pub extern "C" fn grpc_metadata_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.metadata.remove(&args[0]).is_none() { return HOST_STATUS_NOT_FOUND; }
    write_result(ctx, 1)
}

pub extern "C" fn grpc_status_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(code) = grpc_code(args[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(message) = read_spectra_string(args[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, state.statuses.insert(GrpcStatus::new(code, message)))
}

pub extern "C" fn grpc_status_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(status) = state.statuses.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, grpc_code_value(status.code))
}

pub extern "C" fn grpc_status_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(status) = state.statuses.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, alloc_spectra_string(&status.message))
}

pub extern "C" fn grpc_status_set_details_base64(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(encoded) = read_spectra_string(args[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Ok(details) = decode_base64(&encoded) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(status) = state.statuses.get_mut(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    status.details_bin = Some(details);
    write_result(ctx, 1)
}

pub extern "C" fn grpc_status_details_base64(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(status) = state.statuses.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, alloc_spectra_string(&encode_base64(status.details_bin.as_deref().unwrap_or(&[]))))
}

pub extern "C" fn grpc_status_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.statuses.remove(&args[0]).is_none() { return HOST_STATUS_NOT_FOUND; }
    write_result(ctx, 1)
}

pub extern "C" fn grpc_response_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(response) = state.responses.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    let Some(message) = state.messages.get(&response.message).cloned() else { return HOST_STATUS_NOT_FOUND; };
    drop(state);
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, state.messages.insert(message))
}

pub extern "C" fn grpc_response_status(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(status) = state.responses.get(&args[0]).map(|response| response.trailers.status.clone()) else {
        return HOST_STATUS_NOT_FOUND;
    };
    write_result(ctx, state.statuses.insert(status))
}

pub extern "C" fn grpc_response_metadata(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(metadata) = state.responses.get(&args[0]).map(|response| response.trailers.metadata.clone()) else {
        return HOST_STATUS_NOT_FOUND;
    };
    write_result(ctx, state.metadata.insert(metadata))
}

pub extern "C" fn grpc_response_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(response) = state.responses.remove(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    let _ = state.messages.remove(&response.message);
    write_result(ctx, 1)
}

pub extern "C" fn grpc_error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(error) = state.errors.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, error.code)
}

pub extern "C" fn grpc_error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(error) = state.errors.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, alloc_spectra_string(&error.message))
}

pub extern "C" fn grpc_error_details_base64(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(error) = state.errors.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, alloc_spectra_string(&encode_base64(error.details.as_deref().unwrap_or(&[]))))
}

pub extern "C" fn grpc_error_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.errors.remove(&args[0]).is_none() { return HOST_STATUS_NOT_FOUND; }
    write_result(ctx, 1)
}

pub extern "C" fn grpc_client_connect(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(address) = read_spectra_string(args[0]).and_then(|value| value.parse::<SocketAddr>().ok()) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(runtime) = runtime() else { return HOST_STATUS_INTERNAL_ERROR; };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let result = runtime.block_on(with_cancellation(GrpcClient::connect(address), token));
        match result {
            Ok(client) => {
                let entry = ClientEntry {
                    runtime: Arc::clone(&runtime),
                    client: Arc::new(tokio::sync::Mutex::new(client)),
                };
                let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                Ok(state.clients.insert(entry))
            }
            Err(error) => Ok(error_handle(error)),
        }
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

/// TLS variant of `grpc_client_connect`: `roots_base64` carries one DER root
/// certificate (empty string selects no trust anchors, so the handshake
/// fails honestly) and `server_name` must parse as a rustls server name.
/// ALPN offers `h2`; the server side must terminate TLS with the same
/// protocol or the h2 handshake fails loudly instead of degrading.
pub extern "C" fn grpc_client_connect_tls(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(address) = read_spectra_string(args[0]).and_then(|value| value.parse::<SocketAddr>().ok()) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(roots_encoded) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(server_name) = read_spectra_string(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let roots = if roots_encoded.is_empty() {
        Vec::new()
    } else {
        let Ok(der) = decode_base64(&roots_encoded) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        vec![der]
    };
    if rustls::pki_types::ServerName::try_from(server_name.clone()).is_err() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let Ok(runtime) = runtime() else { return HOST_STATUS_INTERNAL_ERROR; };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let result = runtime.block_on(with_cancellation(GrpcClient::connect_tls(address, roots, &server_name), token));
        match result {
            Ok(client) => {
                let entry = ClientEntry {
                    runtime: Arc::clone(&runtime),
                    client: Arc::new(tokio::sync::Mutex::new(client)),
                };
                let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                Ok(state.clients.insert(entry))
            }
            Err(error) => Ok(error_handle(error)),
        }
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

pub extern "C" fn grpc_client_unary(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (client, runtime, message, metadata, path, timeout) = {
        let Some(path) = read_spectra_string(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(metadata) = metadata_handle(args[3]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.clients.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
        let Some(message) = state.messages.get(&args[2]).cloned() else { return HOST_STATUS_NOT_FOUND; };
        (
            Arc::clone(&entry.client),
            Arc::clone(&entry.runtime),
            message,
            metadata,
            path,
            timeout_from_ms(args[4]),
        )
    };
    let future = async move {
        let mut client = client.lock().await;
        client.unary(&path, message, metadata, timeout).await
    };
    let convert = move |(message, trailers): (GrpcMessage, GrpcTrailers)| {
        let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let message_handle = state.messages.insert(message);
        Ok(state.responses.insert(GrpcHostResponse { message: message_handle, trailers }))
    };
    match spawn_grpc_task(runtime, future, convert) {
        Ok(task) => write_result(ctx, task),
        Err(status) => status,
    }
}

pub extern "C" fn grpc_client_client_streaming(ctx: *mut SpectraHostCallContext) -> i32 {
    grpc_client_open_stream(ctx, 0)
}

pub extern "C" fn grpc_client_bidi_streaming(ctx: *mut SpectraHostCallContext) -> i32 {
    grpc_client_open_stream(ctx, 1)
}

pub extern "C" fn grpc_client_server_streaming(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (client, runtime, message, metadata, path, timeout) = {
        let Some(path) = read_spectra_string(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(metadata) = metadata_handle(args[3]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.clients.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
        let Some(message) = state.messages.get(&args[2]).cloned() else { return HOST_STATUS_NOT_FOUND; };
        (
            Arc::clone(&entry.client),
            Arc::clone(&entry.runtime),
            message,
            metadata,
            path,
            timeout_from_ms(args[4]),
        )
    };
    let future = async move {
        let mut client = client.lock().await;
        client.server_streaming(&path, message, metadata, timeout).await
    };
    let stream_runtime = Arc::clone(&runtime);
    let convert = move |receiver: GrpcReceiver| {
        let stream = StreamEntry {
            runtime: stream_runtime,
            inner: Arc::new(tokio::sync::Mutex::new(HostStreamInner::Server(receiver))),
        };
        let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(state.streams.insert(stream))
    };
    match spawn_grpc_task(runtime, future, convert) {
        Ok(task) => write_result(ctx, task),
        Err(status) => status,
    }
}

fn grpc_client_open_stream(ctx: *mut SpectraHostCallContext, bidi: u8) -> i32 {
    let Ok(args) = read_args(ctx, 5) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (client, runtime, path, metadata, timeout) = {
        let Some(path) = read_spectra_string(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(metadata) = metadata_handle(args[3]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.clients.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
        (
            Arc::clone(&entry.client),
            Arc::clone(&entry.runtime),
            path,
            metadata,
            timeout_from_ms(args[4]),
        )
    };
    let future = async move {
        let mut client = client.lock().await;
        if bidi == 0 {
            client.client_streaming(&path, metadata, timeout).await
        } else {
            client.bidi_streaming(&path, metadata, timeout).await
        }
    };
    let stream_runtime = Arc::clone(&runtime);
    let convert = move |stream: GrpcClientStream| {
        let entry = StreamEntry {
            runtime: stream_runtime,
            inner: Arc::new(tokio::sync::Mutex::new(HostStreamInner::Client(stream))),
        };
        let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(state.streams.insert(entry))
    };
    match spawn_grpc_task(runtime, future, convert) {
        Ok(task) => write_result(ctx, task),
        Err(status) => status,
    }
}

pub extern "C" fn grpc_stream_send(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (stream, runtime, message) = {
        let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.streams.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
        let Some(message) = state.messages.get(&args[1]).cloned() else { return HOST_STATUS_NOT_FOUND; };
        (Arc::clone(&entry.inner), Arc::clone(&entry.runtime), message)
    };
    let future = async move {
        let guard = stream.lock().await;
        let sender = match &*guard {
            HostStreamInner::Client(stream) => stream.sender.clone(),
            HostStreamInner::Server(_) => {
                return Err(GrpcError::InvalidPath("server stream is receive-only".to_string()));
            }
        };
        drop(guard);
        sender.send(message).await
    };
    match spawn_grpc_task(runtime, future, |()| Ok(1)) {
        Ok(task) => write_result(ctx, task),
        Err(status) => status,
    }
}

pub extern "C" fn grpc_stream_recv(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (stream, runtime) = {
        let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.streams.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
        (Arc::clone(&entry.inner), Arc::clone(&entry.runtime))
    };
    let future = async move {
        let mut guard = stream.lock().await;
        let item = match &mut *guard {
            HostStreamInner::Client(stream) => stream.recv().await,
            HostStreamInner::Server(receiver) => receiver.recv().await,
        };
        match item {
            None => Ok(None),
            Some(Ok(message)) => Ok(Some(message)),
            Some(Err(error)) => Err(error),
        }
    };
    let convert = |message: Option<GrpcMessage>| {
        let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(message.map(|value| state.messages.insert(value)).unwrap_or(0))
    };
    match spawn_grpc_task(runtime, future, convert) {
        Ok(task) => write_result(ctx, task),
        Err(status) => status,
    }
}

pub extern "C" fn grpc_stream_finish(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (stream, runtime) = {
        let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.streams.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
        (Arc::clone(&entry.inner), Arc::clone(&entry.runtime))
    };
    let future = async move {
        let guard = stream.lock().await;
        match &*guard {
            HostStreamInner::Client(stream) => { stream.finish(); Ok(()) }
            HostStreamInner::Server(_) => Err(GrpcError::InvalidPath("server stream is receive-only".to_string())),
        }
    };
    match spawn_grpc_task(runtime, future, |()| Ok(1)) {
        Ok(task) => write_result(ctx, task),
        Err(status) => status,
    }
}

pub extern "C" fn grpc_stream_cancel(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (stream, runtime) = {
        let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.streams.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
        (Arc::clone(&entry.inner), Arc::clone(&entry.runtime))
    };
    let future = async move {
        let guard = stream.lock().await;
        match &*guard {
            HostStreamInner::Client(stream) => { stream.cancel(); Ok(()) }
            HostStreamInner::Server(receiver) => { receiver.cancel(); Ok(()) }
        }
    };
    match spawn_grpc_task(runtime, future, |()| Ok(1)) {
        Ok(task) => write_result(ctx, task),
        Err(status) => status,
    }
}

pub extern "C" fn grpc_stream_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.streams.remove(&args[0]).is_none() { return HOST_STATUS_NOT_FOUND; }
    write_result(ctx, 1)
}

pub extern "C" fn grpc_server_bind(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(address) = read_spectra_string(args[0]).and_then(|value| value.parse::<SocketAddr>().ok()) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (service, runtime) = {
        let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(service) = state
            .services
            .get(&args[1])
            .cloned()
            .or_else(|| {
                state
                    .callback_services
                    .get(&args[1])
                    .cloned()
                    .map(|service| service as Arc<dyn GrpcService>)
            })
        else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Ok(runtime) = runtime() else { return HOST_STATUS_INTERNAL_ERROR; };
        (service, runtime)
    };
    let max_message_size = usize::try_from(args[2]).ok().filter(|value| *value > 0 && *value <= MAX_HOST_BYTES).unwrap_or(0);
    let stream_capacity = usize::try_from(args[3]).ok().filter(|value| *value > 0 && *value <= 4096).unwrap_or(0);
    let max_concurrent_streams = u32::try_from(args[4]).ok().filter(|value| *value > 0).unwrap_or(0);
    if max_message_size == 0 || stream_capacity == 0 || max_concurrent_streams == 0 { return HOST_STATUS_INVALID_ARGUMENT; }
    let config = GrpcServerConfig { max_message_size, stream_capacity, max_concurrent_streams, tls: None };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let result = runtime.block_on(with_cancellation(GrpcServer::bind(address, service, config), token));
        match result {
            Ok(server) => {
                let entry = ServerEntry {
                    server,
                    runtime: Arc::clone(&runtime),
                };
                let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                Ok(state.servers.insert(entry))
            }
            Err(error) => Ok(error_handle(error)),
        }
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

/// TLS variant of `grpc_server_bind`: `cert_base64`/`key_base64` carry the
/// DER certificate chain (single certificate) and PKCS#8 private key. The
/// identity is validated with the shared rustls builder (ALPN `h2`) before
/// any socket binds, so bad material fails fast with INVALID_ARGUMENT.
pub extern "C" fn grpc_server_bind_tls(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 7) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let Some(address) = read_spectra_string(args[0]).and_then(|value| value.parse::<SocketAddr>().ok()) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (service, runtime) = {
        let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(service) = state
            .services
            .get(&args[1])
            .cloned()
            .or_else(|| {
                state
                    .callback_services
                    .get(&args[1])
                    .cloned()
                    .map(|service| service as Arc<dyn GrpcService>)
            })
        else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Ok(runtime) = runtime() else { return HOST_STATUS_INTERNAL_ERROR; };
        (service, runtime)
    };
    let max_message_size = usize::try_from(args[2]).ok().filter(|value| *value > 0 && *value <= MAX_HOST_BYTES).unwrap_or(0);
    let stream_capacity = usize::try_from(args[3]).ok().filter(|value| *value > 0 && *value <= 4096).unwrap_or(0);
    let max_concurrent_streams = u32::try_from(args[4]).ok().filter(|value| *value > 0).unwrap_or(0);
    if max_message_size == 0 || stream_capacity == 0 || max_concurrent_streams == 0 { return HOST_STATUS_INVALID_ARGUMENT; }
    let (Some(cert), Some(key)) = (
        read_spectra_string(args[5]).and_then(|value| decode_base64(&value).ok()),
        read_spectra_string(args[6]).and_then(|value| decode_base64(&value).ok()),
    ) else { return HOST_STATUS_INVALID_ARGUMENT; };
    if crate::tls::server_config_from_der(vec![cert.clone()], key.clone(), vec![b"h2".to_vec()]).is_err() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let config = GrpcServerConfig { max_message_size, stream_capacity, max_concurrent_streams, tls: Some(GrpcTlsIdentity { cert_chain_der: vec![cert], private_key_der: key }) };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let result = runtime.block_on(with_cancellation(GrpcServer::bind(address, service, config), token));
        match result {
            Ok(server) => {
                let entry = ServerEntry {
                    server,
                    runtime: Arc::clone(&runtime),
                };
                let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                Ok(state.servers.insert(entry))
            }
            Err(error) => Ok(error_handle(error)),
        }
    });
    match task { Ok(task) => write_result(ctx, task), Err(status) => status }
}

pub extern "C" fn grpc_server_local_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(entry) = state.servers.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    write_result(ctx, entry.server.local_addr().port() as SpectraHostValue)
}

pub extern "C" fn grpc_server_shutdown(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(entry) = state.servers.get(&args[0]) else { return HOST_STATUS_NOT_FOUND; };
    entry.server.shutdown();
    write_result(ctx, 1)
}

pub extern "C" fn grpc_server_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.servers.remove(&args[0]).is_none() { return HOST_STATUS_NOT_FOUND; }
    write_result(ctx, 1)
}

/// Mints an empty language-backed service. Methods attach with
/// `grpc_service_handle_method`; the service resolves in `grpc_server_bind`
/// like any registered implementation.
pub extern "C" fn grpc_service_create(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(_args) = read_args(ctx, 0) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, state.callback_services.insert(Arc::new(CallbackGrpcService::new())))
}

/// Attaches one Spectra handler to a service path. The handler is a sync
/// `func(string) returns string` over the service JSON protocol (an async
/// closure's task handle is settled first); anything else fails loudly at
/// call time. Streams are buffered: the full inbound stream feeds a single
/// invocation whose messages stream out.
pub extern "C" fn grpc_service_handle_method(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let (Some(path), closure) = (read_spectra_string(args[1]), args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if path.is_empty() || closure == 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let Some(invoke) = (unsafe { ctx.as_ref() }).and_then(|ctx| ctx.invoke_fn) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(service) = state.callback_services.get(&args[0]).cloned() else {
        return HOST_STATUS_NOT_FOUND;
    };
    service
        .methods
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(path, crate::handler::CallbackEntry { closure, invoke });
    write_result(ctx, 1)
}

pub extern "C" fn grpc_service_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return HOST_STATUS_INVALID_ARGUMENT; };
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, i64::from(state.callback_services.remove(&args[0]).is_some()))
}

/// Registers a real service implementation for `grpc_server_bind`.
///
/// Rust-side entry point used by `service_registry_tests`. Language callers
/// mint services with `grpc_service_create` instead; `grpc_server_bind` and
/// `grpc_server_bind_tls` resolve handles through both the services table
/// and the language-service table. No in-process fake transport is involved.
#[cfg(test)]
pub fn grpc_server_register_service(service: Arc<dyn GrpcService>) -> SpectraHostValue {
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state.services.insert(service)
}

#[cfg(test)]
pub fn grpc_server_unregister_service(handle: SpectraHostValue) -> bool {
    let mut state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state.services.remove(&handle).is_some()
}

#[cfg(test)]
mod service_registry_tests {
    use super::*;
    use crate::grpc::{GrpcError, GrpcRequest};

    #[test]
    fn registered_service_resolves_for_bind_and_unregisters() {
        let service: Arc<dyn GrpcService> =
            Arc::new(|_request: GrpcRequest| async move { Err(GrpcError::Cancelled) });
        let handle = grpc_server_register_service(service);
        assert!(handle > 0, "registration must mint a live handle");
        {
            let state = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(
                state.services.get(&handle).is_some(),
                "bind resolves the registered handle through the services table"
            );
        }
        assert!(grpc_server_unregister_service(handle));
        assert!(
            !grpc_server_unregister_service(handle),
            "double unregister must report missing"
        );
    }
}

#[cfg(test)]
mod transport_isolation_tests {
    use super::*;
    use crate::grpc::{GrpcError, GrpcRequest};

    #[test]
    fn rust_level_unary_roundtrip() {
        let runtime = runtime().expect("runtime");
        let service: Arc<dyn GrpcService> =
            Arc::new(|request: GrpcRequest| async move {
                let mut messages = Vec::new();
                let mut inbound = request.inbound;
                while let Some(item) = inbound.recv().await {
                    messages.push(item.map_err(|e| GrpcError::Protocol(e.to_string()))?);
                }
                assert_eq!(messages.len(), 1);
                Ok(GrpcResponse::unary(messages.into_iter().next().unwrap()))
            });
        let server = runtime
            .block_on(GrpcServer::bind(
                "127.0.0.1:0".parse().unwrap(),
                service,
                GrpcServerConfig::default(),
            ))
            .expect("bind");
        let address = server.local_addr();
        let mut client = runtime
            .block_on(crate::grpc::GrpcClient::connect(address))
            .expect("connect");
        let (message, trailers) = runtime
            .block_on(client.unary(
                "/echo.Echo/Run",
                GrpcMessage::from_bytes(b"hello".to_vec()),
                GrpcMetadata::new(),
                Some(Duration::from_secs(5)),
            ))
            .expect("unary");
        assert_eq!(message.as_bytes(), b"hello");
        assert_eq!(trailers.status.code, GrpcCode::Ok);
    }
}
