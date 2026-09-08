//! Host-call adapters for the real GraphQL protocol core.
//!
//! This module intentionally does not use the shared runtime handle catalog yet.  Its
//! local handles use a reserved, tagged encoding and retain the same generation
//! invariant as `HandleTable`: releasing a slot increments its generation, so stale
//! values can never name a later object.

use crate::graphql::{
    self, GraphqlField, GraphqlRequest, GraphqlRoot, GraphqlSchema, GraphqlSchemaBuilder,
    GraphqlSubscription, GraphqlSubscriptionCallback, GraphqlSubscriptionField, ResolverInput,
    Value,
};
use crate::handler::{invoke_callback, CallbackEntry};
use crate::http::Method;
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use futures_util::stream;
use serde_json::{json, Value as JsonValue};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use std::future::Future;
use std::sync::{Mutex, OnceLock};
use std::thread;
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

const GRAPHQL_HANDLE_PREFIX: u16 = 0x7f00;
const BUILDER_TAG: u16 = 1;
const SCHEMA_TAG: u16 = 2;
const SUBSCRIPTION_TAG: u16 = 3;
const RESPONSE_TAG: u16 = 4;
const MAX_GRAPHQL_HANDLES: usize = 16_384;

struct LocalSlot<T> {
    generation: u16,
    value: Option<T>,
}

struct LocalHandleTable<T> {
    tag: u16,
    slots: Vec<LocalSlot<T>>,
    free: Vec<u32>,
}

impl<T> LocalHandleTable<T> {
    fn new(tag: u16) -> Self {
        Self { tag, slots: Vec::new(), free: Vec::new() }
    }

    fn insert(&mut self, value: T) -> Option<SpectraHostValue> {
        let slot = if let Some(slot) = self.free.pop() {
            slot
        } else {
            if self.slots.len() >= MAX_GRAPHQL_HANDLES || self.slots.len() > u32::MAX as usize {
                return None;
            }
            let slot = self.slots.len() as u32;
            self.slots.push(LocalSlot { generation: 1, value: None });
            slot
        };
        let generation = self.slots[slot as usize].generation;
        self.slots[slot as usize].value = Some(value);
        Some(self.encode(slot, generation))
    }

    fn encode(&self, slot: u32, generation: u16) -> SpectraHostValue {
        let raw = (u64::from(GRAPHQL_HANDLE_PREFIX | self.tag) << 48)
            | (u64::from(generation) << 32)
            | (u64::from(slot) + 1);
        raw as SpectraHostValue
    }

    fn decode(&self, raw: SpectraHostValue) -> Option<(usize, u16)> {
        if raw <= 0 {
            return None;
        }
        let value = raw as u64;
        if (value >> 48) as u16 != (GRAPHQL_HANDLE_PREFIX | self.tag) {
            return None;
        }
        let generation = ((value >> 32) & 0xffff) as u16;
        let encoded_slot = (value & 0xffff_ffff) as u32;
        if generation == 0 || encoded_slot == 0 {
            return None;
        }
        Some(((encoded_slot - 1) as usize, generation))
    }

    fn get(&self, raw: SpectraHostValue) -> Option<&T> {
        let (slot, generation) = self.decode(raw)?;
        let entry = self.slots.get(slot)?;
        if entry.generation != generation { return None; }
        entry.value.as_ref()
    }

    fn get_mut(&mut self, raw: SpectraHostValue) -> Option<&mut T> {
        let (slot, generation) = self.decode(raw)?;
        let entry = self.slots.get_mut(slot)?;
        if entry.generation != generation { return None; }
        entry.value.as_mut()
    }

    fn remove(&mut self, raw: SpectraHostValue) -> Option<T> {
        let (slot, generation) = self.decode(raw)?;
        let entry = self.slots.get_mut(slot)?;
        if entry.generation != generation { return None; }
        let value = entry.value.take()?;
        entry.generation = entry.generation.wrapping_add(1).max(1);
        self.free.push(slot as u32);
        Some(value)
    }
}

struct ResponseEntry {
    json: String,
    status: SpectraHostValue,
}

struct GraphqlStore {
    builders: LocalHandleTable<GraphqlSchemaBuilder>,
    schemas: LocalHandleTable<GraphqlSchema>,
    subscriptions: LocalHandleTable<std::sync::Arc<GraphqlSubscription>>,
    responses: LocalHandleTable<ResponseEntry>,
}

impl GraphqlStore {
    fn new() -> Self {
        Self {
            builders: LocalHandleTable::new(BUILDER_TAG),
            schemas: LocalHandleTable::new(SCHEMA_TAG),
            subscriptions: LocalHandleTable::new(SUBSCRIPTION_TAG),
            responses: LocalHandleTable::new(RESPONSE_TAG),
        }
    }
}

fn store() -> &'static Mutex<GraphqlStore> {
    static STORE: OnceLock<Mutex<GraphqlStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(GraphqlStore::new()))
}

fn invalid() -> i32 { HOST_STATUS_INVALID_ARGUMENT }

fn root(raw: SpectraHostValue) -> Option<GraphqlRoot> {
    match raw {
        0 => Some(GraphqlRoot::Query),
        1 => Some(GraphqlRoot::Mutation),
        _ => None,
    }
}

fn graphql_type(name: String) -> Option<graphql::TypeRef> {
    if name.is_empty() || name.len() > 256 || name.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return None;
    }
    let mut offset = 0;
    let value = parse_graphql_type_ref(&name, &mut offset)?;
    (offset == name.len()).then_some(value)
}

fn parse_graphql_type_ref(input: &str, offset: &mut usize) -> Option<graphql::TypeRef> {
    let bytes = input.as_bytes();
    let mut value = if bytes.get(*offset) == Some(&b'[') {
        *offset += 1;
        let inner = parse_graphql_type_ref(input, offset)?;
        if bytes.get(*offset) != Some(&b']') {
            return None;
        }
        *offset += 1;
        graphql::TypeRef::List(Box::new(inner))
    } else {
        let start = *offset;
        while bytes
            .get(*offset)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            *offset += 1;
        }
        (start != *offset).then(|| graphql::TypeRef::named(&input[start..*offset]))?
    };
    if bytes.get(*offset) == Some(&b'!') {
        *offset += 1;
        value = graphql::TypeRef::NonNull(Box::new(value));
    }
    Some(value)
}

fn json_to_graphql(value: JsonValue) -> Option<Value> {
    Value::from_json(value).ok()
}

fn callback_input(input: &ResolverInput) -> Option<SpectraHostValue> {
    let payload = serde_json::to_string(&json!({
        "field": input.field_name,
        "arguments": input.arguments,
        "parent": input.parent,
    })).ok()?;
    Some(alloc_spectra_string(&payload))
}

fn resolver_from_callback(callback: CallbackEntry) -> graphql::ResolverCallback {
    graphql::resolver_callback(move |input: ResolverInput| {
        let callback = callback;
        async move {
            let request = callback_input(&input)
                .ok_or_else(|| graphql::Error::new("failed to encode GraphQL resolver input"))?;
            let result = invoke_callback(callback, request)
                .map_err(|status| graphql::Error::new(format!("GraphQL resolver callback failed (status {status})")))?;
            let encoded = read_spectra_string(result)
                .ok_or_else(|| graphql::Error::new("GraphQL resolver callback must return a JSON string"))?;
            let value = serde_json::from_str::<JsonValue>(&encoded)
                .map_err(|error| graphql::Error::new(format!("GraphQL resolver returned invalid JSON: {error}")))?;
            let value = json_to_graphql(value)
                .ok_or_else(|| graphql::Error::new("GraphQL resolver result cannot be represented as GraphQL value"))?;
            Ok(Some(value))
        }
    })
}

fn subscription_from_callback(callback: CallbackEntry) -> GraphqlSubscriptionCallback {
    graphql::subscription_callback(move |input: ResolverInput| {
        let callback = callback;
        async move {
            let request = callback_input(&input)
                .ok_or_else(|| graphql::Error::new("failed to encode GraphQL subscription input"))?;
            let result = invoke_callback(callback, request)
                .map_err(|status| graphql::Error::new(format!("GraphQL subscription callback failed (status {status})")))?;
            let encoded = read_spectra_string(result)
                .ok_or_else(|| graphql::Error::new("GraphQL subscription callback must return a JSON array string"))?;
            let values = serde_json::from_str::<Vec<JsonValue>>(&encoded)
                .map_err(|error| graphql::Error::new(format!("GraphQL subscription returned invalid JSON array: {error}")))?;
            let values = values.into_iter().map(|value| {
                json_to_graphql(value).ok_or_else(|| graphql::Error::new("GraphQL subscription event cannot be represented as GraphQL value"))
            }).collect::<Result<Vec<_>, _>>()?;
            Ok(stream::iter(values.into_iter().map(Ok)))
        }
    })
}

fn callback_entry(ctx: *mut SpectraHostCallContext, closure: SpectraHostValue) -> Option<CallbackEntry> {
    if closure == 0 { return None; }
    let invoke = unsafe { ctx.as_ref() }?.invoke_fn?;
    Some(CallbackEntry { closure, invoke })
}

fn parse_variables(raw: SpectraHostValue) -> Option<JsonValue> {
    let encoded = read_spectra_string(raw)?;
    let value = serde_json::from_str::<JsonValue>(&encoded).ok()?;
    if value.is_object() { Some(value) } else { None }
}

fn parse_request(query: SpectraHostValue, variables: SpectraHostValue, operation: Option<SpectraHostValue>) -> Option<GraphqlRequest> {
    let query = read_spectra_string(query)?;
    if query.trim().is_empty() { return None; }
    let variables = parse_variables(variables)?;
    let mut request = GraphqlRequest::new(query).variables(variables);
    if let Some(operation) = operation {
        let operation = read_spectra_string(operation)?;
        if !operation.is_empty() { request = request.operation_name(operation); }
    }
    Some(request)
}

fn insert_response(response: GraphqlResponseOwned, ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(handle) = store.responses.insert(ResponseEntry { json: response.json, status: response.status }) else { return invalid(); };
    write_result(ctx, handle)
}

struct GraphqlResponseOwned { json: String, status: SpectraHostValue }

fn execute_on_runtime<F>(future: F) -> Option<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        thread::spawn(move || {
            RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .map(|runtime| runtime.block_on(future))
        })
        .join()
        .ok()
        .and_then(Result::ok)
    } else {
        RuntimeBuilder::new_current_thread().enable_all().build().ok().map(|runtime| runtime.block_on(future))
    }
}

fn background_runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| RuntimeBuilder::new_multi_thread().worker_threads(2).enable_all().build().expect("GraphQL runtime"))
}

fn open_subscription(schema: GraphqlSchema, request: GraphqlRequest) -> Option<GraphqlSubscription> {
    if tokio::runtime::Handle::try_current().is_ok() {
        Some(schema.subscribe(request))
    } else {
        Some(background_runtime().block_on(async move { schema.subscribe(request) }))
    }
}

fn get_schema(raw: SpectraHostValue) -> Option<GraphqlSchema> {
    store().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).schemas.get(raw).cloned()
}

fn update_builder<F>(raw: SpectraHostValue, update: F) -> bool
where
    F: FnOnce(GraphqlSchemaBuilder) -> GraphqlSchemaBuilder,
{
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(builder) = store.builders.get_mut(raw) else { return false; };
    let current = std::mem::replace(builder, GraphqlSchemaBuilder::new());
    *builder = update(current);
    true
}

/// Create a schema builder. Handle is owned by the host until `schema_finish` or
/// `schema_drop` consumes it.
pub extern "C" fn schema_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 0) else { return invalid(); };
    let _ = args;
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(handle) = store.builders.insert(GraphqlSchemaBuilder::new()) else { return invalid(); };
    write_result(ctx, handle)
}

pub extern "C" fn schema_set_workers(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return invalid(); };
    let Ok(concurrency) = usize::try_from(args[1]) else { return invalid(); };
    if concurrency == 0 { return invalid(); }
    if !update_builder(args[0], |builder| builder.workers(graphql::GraphqlWorkerExecutor::new(concurrency))) { return invalid(); }
    write_result(ctx, 1)
}

pub extern "C" fn schema_set_subscription_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else { return invalid(); };
    let Ok(capacity) = usize::try_from(args[1]) else { return invalid(); };
    if capacity == 0 { return invalid(); }
    if !update_builder(args[0], |builder| builder.subscription_capacity(capacity)) { return invalid(); }
    write_result(ctx, 1)
}

pub extern "C" fn schema_field_json(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else { return invalid(); };
    let Some(root) = root(args[1]) else { return invalid(); };
    let (Some(name), Some(type_name), Some(encoded)) = (read_spectra_string(args[2]), read_spectra_string(args[3]), read_spectra_string(args[4])) else { return invalid(); };
    let Ok(value) = serde_json::from_str::<JsonValue>(&encoded) else { return invalid(); };
    let Some(value) = json_to_graphql(value) else { return invalid(); };
    let Some(ty) = graphql_type(type_name) else { return invalid(); };
    if !update_builder(args[0], |builder| builder.field(root, GraphqlField::new(name, ty, graphql::static_value_resolver(value)))) { return invalid(); }
    write_result(ctx, 1)
}

pub extern "C" fn schema_field_callback(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else { return invalid(); };
    let Some(root) = root(args[1]) else { return invalid(); };
    let (Some(name), Some(ty), Some(callback)) = (read_spectra_string(args[2]), read_spectra_string(args[3]).and_then(graphql_type), callback_entry(ctx, args[4])) else { return invalid(); };
    if !update_builder(args[0], |builder| builder.field(root, GraphqlField::new(name, ty, resolver_from_callback(callback)))) { return invalid(); }
    write_result(ctx, 1)
}

pub extern "C" fn schema_subscription_json(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 4) else { return invalid(); };
    let (Some(name), Some(type_name), Some(encoded)) = (read_spectra_string(args[1]), read_spectra_string(args[2]), read_spectra_string(args[3])) else { return invalid(); };
    let Ok(values) = serde_json::from_str::<Vec<JsonValue>>(&encoded) else { return invalid(); };
    let Some(ty) = graphql_type(type_name) else { return invalid(); };
    let values = values.into_iter().map(json_to_graphql).collect::<Option<Vec<_>>>();
    let Some(values) = values else { return invalid(); };
    let callback = graphql::subscription_callback(move |_| {
        let values = values.clone();
        async move { Ok(stream::iter(values.into_iter().map(Ok))) }
    });
    if !update_builder(args[0], |builder| builder.subscription_field(GraphqlSubscriptionField::new(name, ty, callback))) { return invalid(); }
    write_result(ctx, 1)
}

pub extern "C" fn schema_subscription_callback(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 4) else { return invalid(); };
    let (Some(name), Some(ty), Some(callback)) = (read_spectra_string(args[1]), read_spectra_string(args[2]).and_then(graphql_type), callback_entry(ctx, args[3])) else { return invalid(); };
    if !update_builder(args[0], |builder| builder.subscription_field(GraphqlSubscriptionField::new(name, ty, subscription_from_callback(callback)))) { return invalid(); }
    write_result(ctx, 1)
}

pub extern "C" fn schema_finish(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let Some(builder) = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).builders.remove(args[0]) else { return invalid(); };
    let Ok(schema) = builder.finish() else { return invalid(); };
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(handle) = store.schemas.insert(schema) else { return invalid(); };
    write_result(ctx, handle)
}

pub extern "C" fn schema_drop(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if store.schemas.remove(args[0]).is_some() || store.builders.remove(args[0]).is_some() { write_result(ctx, 1) } else { invalid() }
}

pub extern "C" fn schema_sdl(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let Some(schema) = get_schema(args[0]) else { return invalid(); };
    write_result(ctx, alloc_spectra_string(&schema.sdl()))
}

fn execute_request(ctx: *mut SpectraHostCallContext, request: GraphqlRequest) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let Some(schema) = get_schema(args[0]) else { return invalid(); };
    let Some(response) = execute_on_runtime(async move { schema.execute(request).await }) else { return invalid(); };
    let Ok(json) = String::from_utf8(response.json_bytes()) else { return invalid(); };
    insert_response(GraphqlResponseOwned { json, status: 200 }, ctx)
}

pub extern "C" fn execute(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return invalid(); };
    let Some(request) = parse_request(args[1], args[2], None) else { return invalid(); };
    execute_request(ctx, request)
}

pub extern "C" fn execute_named(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 4) else { return invalid(); };
    let Some(request) = parse_request(args[1], args[2], Some(args[3])) else { return invalid(); };
    execute_request(ctx, request)
}

pub extern "C" fn execute_http(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else { return invalid(); };
    let Some(schema) = get_schema(args[0]) else { return invalid(); };
    let Some(method) = Method::from_code(args[1]) else { return invalid(); };
    let Some(target) = read_spectra_string(args[2]) else { return invalid(); };
    let Some(content_type) = read_spectra_string(args[3]) else { return invalid(); };
    let Some(body) = read_spectra_string(args[4]) else { return invalid(); };
    let mut request = match ::http::Request::builder().method(method.as_str()).uri(target).body(body.into_bytes()) {
        Ok(request) => request,
        Err(_) => return invalid(),
    };
    let Ok(header) = ::http::HeaderValue::from_str(&content_type) else { return invalid(); };
    request.headers_mut().insert(::http::header::CONTENT_TYPE, header);
    let Some(response) = execute_on_runtime(async move { schema.execute_http(request).await }) else { return invalid(); };
    let Ok(json) = String::from_utf8(response.body().clone()) else { return invalid(); };
    let status = SpectraHostValue::from(response.status().as_u16());
    insert_response(GraphqlResponseOwned { json, status }, ctx)
}

pub extern "C" fn response_json(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(response) = store.responses.get(args[0]) else { return invalid(); };
    write_result(ctx, alloc_spectra_string(&response.json))
}

pub extern "C" fn response_status(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(response) = store.responses.get(args[0]) else { return invalid(); };
    write_result(ctx, response.status)
}

fn response_value(raw: SpectraHostValue) -> Option<JsonValue> {
    let store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    serde_json::from_str(&store.responses.get(raw)?.json).ok()
}

pub extern "C" fn response_is_ok(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let Some(value) = response_value(args[0]) else { return invalid(); };
    let is_ok = value.get("errors")
        .map(|errors| errors.as_array().is_some_and(Vec::is_empty))
        .unwrap_or(true);
    write_result(ctx, is_ok as SpectraHostValue)
}

pub extern "C" fn response_errors_json(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let Some(value) = response_value(args[0]) else { return invalid(); };
    let errors = value.get("errors").cloned().unwrap_or_else(|| json!([]));
    let Ok(encoded) = serde_json::to_string(&errors) else { return invalid(); };
    write_result(ctx, alloc_spectra_string(&encoded))
}

pub extern "C" fn response_data_json(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let Some(value) = response_value(args[0]) else { return invalid(); };
    let data = value.get("data").cloned().unwrap_or(JsonValue::Null);
    let Ok(encoded) = serde_json::to_string(&data) else { return invalid(); };
    write_result(ctx, alloc_spectra_string(&encoded))
}

pub extern "C" fn response_drop(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if store.responses.remove(args[0]).is_some() { write_result(ctx, 1) } else { invalid() }
}

pub extern "C" fn subscribe(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else { return invalid(); };
    let Some(schema) = get_schema(args[0]) else { return invalid(); };
    let Some(request) = parse_request(args[1], args[2], None) else { return invalid(); };
    let Some(subscription) = open_subscription(schema, request) else { return invalid(); };
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(handle) = store.subscriptions.insert(std::sync::Arc::new(subscription)) else { return invalid(); };
    write_result(ctx, handle)
}

pub extern "C" fn subscription_next(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let subscription = {
        let store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        store.subscriptions.get(args[0]).cloned()
    };
    let Some(subscription) = subscription else { return invalid(); };
    let Some(response) = execute_on_runtime(async move { subscription.next().await }) else { return invalid(); };
    let Some(response) = response else { return write_result(ctx, 0); };
    let Ok(json) = String::from_utf8(response.json_bytes()) else { return invalid(); };
    insert_response(GraphqlResponseOwned { json, status: 200 }, ctx)
}

pub extern "C" fn subscription_pending(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(subscription) = store.subscriptions.get(args[0]) else { return invalid(); };
    write_result(ctx, subscription.pending() as SpectraHostValue)
}

pub extern "C" fn subscription_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(subscription) = store.subscriptions.get(args[0]) else { return invalid(); };
    write_result(ctx, subscription.capacity() as SpectraHostValue)
}

pub extern "C" fn subscription_is_cancelled(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(subscription) = store.subscriptions.get(args[0]) else { return invalid(); };
    write_result(ctx, subscription.is_cancelled() as SpectraHostValue)
}

pub extern "C" fn subscription_cancel(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(subscription) = store.subscriptions.get(args[0]) else { return invalid(); };
    subscription.cancel();
    write_result(ctx, 1)
}

pub extern "C" fn subscription_drop(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else { return invalid(); };
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if store.subscriptions.remove(args[0]).is_some() { write_result(ctx, 1) } else { invalid() }
}
