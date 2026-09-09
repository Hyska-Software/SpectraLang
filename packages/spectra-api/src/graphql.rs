//! Dynamic GraphQL schema, execution, subscriptions, and HTTP adaptation.
//!
//! This module deliberately keeps GraphQL parsing, validation, execution, and
//! introspection in `async-graphql`.  The adapter owns only the runtime-safe
//! callback boundary and the bounded subscription transport.

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
pub use async_graphql::{Error, PathSegment, QueryPathNode, ServerError, Value};
use async_graphql::{Request, Response, Variables};
pub use async_graphql::dynamic::{
    Enum, Field, FieldFuture, InputObject, InputValue, Interface, Object, ResolverContext, Scalar,
    Schema, Subscription, SubscriptionField, SubscriptionFieldFuture, Type, TypeRef, Union,
};
use futures_util::{Stream, StreamExt, future::BoxFuture, stream::BoxStream};
use http::{HeaderValue, Method, Request as HttpRequest, Response as HttpResponse, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{sync::{Mutex, Notify, Semaphore, mpsc}, task::JoinError};

/// A request-scoped context shared with every resolver invocation.
#[derive(Clone, Debug, Default)]
pub struct GraphqlContext {
    values: Arc<serde_json::Map<String, serde_json::Value>>,
}

impl GraphqlContext {
    /// Create an empty request context.
    pub fn new() -> Self { Self::default() }

    /// Return a context value, if present.
    pub fn get(&self, name: &str) -> Option<&serde_json::Value> { self.values.get(name) }

    /// Return a new context containing `name` and `value`.
    #[must_use]
    pub fn with_value(mut self, name: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        Arc::make_mut(&mut self.values).insert(name.into(), value.into());
        self
    }
}

/// Input made available to a worker-safe resolver callback.
#[derive(Clone, Debug)]
pub struct ResolverInput {
    pub field_name: String,
    pub arguments: serde_json::Value,
    pub parent: serde_json::Value,
    pub context: Arc<GraphqlContext>,
}

/// The result of a resolver callback. `None` is a GraphQL null result.
pub type ResolverResult = Result<Option<Value>, Error>;
/// A callback future. It is owned and `'static`, so it can leave the HTTP task.
pub type ResolverFuture = Pin<Box<dyn Future<Output = ResolverResult> + Send + 'static>>;
/// A resolver callback safe to invoke on a worker executor.
pub type ResolverCallback = Arc<dyn Fn(ResolverInput) -> ResolverFuture + Send + Sync + 'static>;
#[derive(Clone, Default)]
struct ErrorPathStore {
    paths: Arc<Mutex<HashMap<String, VecDeque<Vec<PathSegment>>>>>,
}

impl ErrorPathStore {
    async fn record(&self, message: String, path: Vec<PathSegment>) {
        self.paths.lock().await.entry(message).or_default().push_back(path);
    }

    async fn apply(&self, response: &mut Response) {
        let mut paths = self.paths.lock().await;
        for error in &mut response.errors {
            if error.path.is_empty() {
                if let Some(queue) = paths.get_mut(&error.message) {
                    if let Some(path) = queue.pop_front() {
                        error.path = path;
                    }
                }
            }
        }
    }
}

fn path_from_node(node: QueryPathNode<'_>) -> Vec<PathSegment> {
    node.to_string_vec().into_iter().map(|segment| {
        segment.parse::<usize>().map(PathSegment::Index).unwrap_or(PathSegment::Field(segment))
    }).collect()
}


/// Turn an async closure into a worker-safe callback.
pub fn resolver_callback<F, Fut>(callback: F) -> ResolverCallback
where
    F: Fn(ResolverInput) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ResolverResult> + Send + 'static,
{
    Arc::new(move |input| Box::pin(callback(input)))
}

/// A deterministic resolver useful for schema constants and health fields.
pub fn static_value_resolver(value: Value) -> ResolverCallback {
    Arc::new(move |_| {
        let value = value.clone();
        Box::pin(async move { Ok(Some(value)) })
    })
}

/// A bounded worker executor for resolver callbacks.
#[derive(Clone)]
pub struct GraphqlWorkerExecutor {
    permits: Arc<Semaphore>,
}

impl fmt::Debug for GraphqlWorkerExecutor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraphqlWorkerExecutor").finish_non_exhaustive()
    }
}

impl Default for GraphqlWorkerExecutor {
    fn default() -> Self { Self::new(32) }
}

impl GraphqlWorkerExecutor {
    /// Create an executor with a bounded number of concurrently running callbacks.
    pub fn new(max_concurrency: usize) -> Self {
        assert!(max_concurrency > 0, "GraphQL worker concurrency must be non-zero");
        Self { permits: Arc::new(Semaphore::new(max_concurrency)) }
    }

    fn dispatch(&self, callback: ResolverCallback, input: ResolverInput) -> ResolverFuture {
        let permits = self.permits.clone();
        Box::pin(async move {
            let permit = permits.acquire_owned().await.map_err(|_| Error::new("GraphQL worker executor closed"))?;
            let handle = tokio::runtime::Handle::try_current()
                .map_err(|_| Error::new("GraphQL execution requires a Tokio runtime"))?;
            let join = tokio::task::spawn_blocking(move || handle.block_on(callback(input))).await;
            drop(permit);
            match join {
                Ok(result) => result,
                Err(err) => Err(worker_join_error(err)),
            }
        })
    }

    fn dispatch_subscription(
        &self,
        callback: GraphqlSubscriptionCallback,
        input: ResolverInput,
    ) -> BoxFuture<'static, Result<SubscriptionStream, Error>> {
        let permits = self.permits.clone();
        Box::pin(async move {
            let permit = permits.acquire_owned().await.map_err(|_| Error::new("GraphQL worker executor closed"))?;
            let handle = tokio::runtime::Handle::try_current()
                .map_err(|_| Error::new("GraphQL execution requires a Tokio runtime"))?;
            let join = tokio::task::spawn_blocking(move || handle.block_on(callback(input))).await;
            drop(permit);
            match join {
                Ok(result) => result,
                Err(err) => Err(worker_join_error(err)),
            }
        })
    }
}

fn worker_join_error(err: JoinError) -> Error {
    if err.is_cancelled() { Error::new("GraphQL resolver worker cancelled") }
    else { Error::new(format!("GraphQL resolver worker failed: {err}")) }
}

/// A field to be attached to a query or mutation object.
pub struct GraphqlField {
    name: String,
    ty: TypeRef,
    arguments: Vec<InputValue>,
    resolver: ResolverCallback,
}

impl GraphqlField {
    pub fn new(name: impl Into<String>, ty: impl Into<TypeRef>, resolver: ResolverCallback) -> Self {
        Self { name: name.into(), ty: ty.into(), arguments: Vec::new(), resolver }
    }

    #[must_use]
    pub fn argument(mut self, argument: InputValue) -> Self {
        self.arguments.push(argument);
        self
    }
}

/// A subscription field callback creates an owned stream of events.
pub type SubscriptionStream = BoxStream<'static, Result<Value, Error>>;
pub type GraphqlSubscriptionCallback = Arc<dyn Fn(ResolverInput) -> Pin<Box<dyn Future<Output = Result<SubscriptionStream, Error>> + Send + 'static>> + Send + Sync + 'static>;

pub fn subscription_callback<F, Fut, S>(callback: F) -> GraphqlSubscriptionCallback
where
    F: Fn(ResolverInput) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<S, Error>> + Send + 'static,
    S: Stream<Item = Result<Value, Error>> + Send + 'static,
{
    Arc::new(move |input| {
        let future = callback(input);
        Box::pin(async move { Ok(future.await?.boxed()) })
    })
}

pub struct GraphqlSubscriptionField {
    name: String,
    ty: TypeRef,
    arguments: Vec<InputValue>,
    resolver: GraphqlSubscriptionCallback,
}

impl GraphqlSubscriptionField {
    pub fn new(name: impl Into<String>, ty: impl Into<TypeRef>, resolver: GraphqlSubscriptionCallback) -> Self {
        Self { name: name.into(), ty: ty.into(), arguments: Vec::new(), resolver }
    }

    #[must_use]
    pub fn argument(mut self, argument: InputValue) -> Self {
        self.arguments.push(argument);
        self
    }
}

/// Root to which a field is attached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphqlRoot { Query, Mutation, Subscription }

/// Schema construction errors are emitted by async-graphql's strict registry checks.
pub type GraphqlSchemaError = async_graphql::dynamic::SchemaError;

/// Builder for an owned dynamic GraphQL schema.
pub struct GraphqlSchemaBuilder {
    query: Object,
    mutation: Option<Object>,
    subscription: Option<Subscription>,
    types: Vec<Type>,
    context: Arc<GraphqlContext>,
    workers: GraphqlWorkerExecutor,
    subscription_capacity: usize,
    max_depth: Option<usize>,
    max_complexity: Option<usize>,
    introspection: bool,
}

impl GraphqlSchemaBuilder {
    pub fn new() -> Self { Self::default() }

    #[must_use]
    pub fn context(mut self, context: Arc<GraphqlContext>) -> Self { self.context = context; self }

    #[must_use]
    pub fn workers(mut self, workers: GraphqlWorkerExecutor) -> Self { self.workers = workers; self }

    #[must_use]
    pub fn subscription_capacity(mut self, capacity: usize) -> Self {
        assert!(capacity > 0, "GraphQL subscription capacity must be non-zero");
        self.subscription_capacity = capacity;
        self
    }
    /// Cap query depth (`None` = unlimited, the default). Depth is counted
    /// the async-graphql way: every nested field level adds one.
    #[must_use]
    pub fn max_depth(mut self, depth: Option<usize>) -> Self {
        self.max_depth = depth.filter(|value| *value > 0);
        self
    }

    /// Cap query complexity (`None` = unlimited, the default).
    #[must_use]
    pub fn max_complexity(mut self, complexity: Option<usize>) -> Self {
        self.max_complexity = complexity.filter(|value| *value > 0);
        self
    }

    /// Toggle introspection (`true` = on, the default). Disabled schemas
    /// reject `__schema`/`__type` queries.
    #[must_use]
    pub fn introspection(mut self, enabled: bool) -> Self {
        self.introspection = enabled;
        self
    }

    #[must_use]
    pub fn register_type(mut self, ty: impl Into<Type>) -> Self { self.types.push(ty.into()); self }
    #[must_use]
    pub fn register_scalar(self, ty: Scalar) -> Self { self.register_type(ty) }
    #[must_use]
    pub fn register_object(self, ty: Object) -> Self { self.register_type(ty) }
    #[must_use]
    pub fn register_input_object(self, ty: InputObject) -> Self { self.register_type(ty) }
    #[must_use]
    pub fn register_enum(self, ty: Enum) -> Self { self.register_type(ty) }
    #[must_use]
    pub fn register_interface(self, ty: Interface) -> Self { self.register_type(ty) }
    #[must_use]
    pub fn register_union(self, ty: Union) -> Self { self.register_type(ty) }

    #[must_use]
    pub fn field(mut self, root: GraphqlRoot, field: GraphqlField) -> Self {
        let dynamic = dynamic_field(field, self.workers.clone(), self.context.clone());
        match root {
            GraphqlRoot::Query => self.query = self.query.field(dynamic),
            GraphqlRoot::Mutation => {
                let object = self.mutation.take().unwrap_or_else(|| Object::new("Mutation"));
                self.mutation = Some(object.field(dynamic));
            }
            GraphqlRoot::Subscription => panic!("use subscription_field for subscription roots"),
        }
        self
    }

    #[must_use]
    pub fn subscription_field(mut self, field: GraphqlSubscriptionField) -> Self {
        let dynamic = dynamic_subscription_field(field, self.workers.clone(), self.context.clone());
        let subscription = self.subscription.take().unwrap_or_else(|| Subscription::new("Subscription"));
        self.subscription = Some(subscription.field(dynamic));
        self
    }

    pub fn finish(self) -> Result<GraphqlSchema, GraphqlSchemaError> {
        let mut builder = Schema::build(
            self.query.type_name(),
            self.mutation.as_ref().map(Object::type_name),
            self.subscription.as_ref().map(Subscription::type_name),
        );
        if let Some(depth) = self.max_depth {
            builder = builder.limit_depth(depth);
        }
        if let Some(complexity) = self.max_complexity {
            builder = builder.limit_complexity(complexity);
        }
        if !self.introspection {
            builder = builder.disable_introspection();
        }
        for ty in self.types { builder = builder.register(ty); }
        let schema = builder.register(self.query);
        let schema = if let Some(mutation) = self.mutation { schema.register(mutation) } else { schema };
        let schema = if let Some(subscription) = self.subscription { schema.register(subscription) } else { schema };
        Ok(GraphqlSchema {
            schema: schema.finish()?,
            context: self.context,
            workers: self.workers,
            subscription_capacity: self.subscription_capacity,
        })
    }
}

impl Default for GraphqlSchemaBuilder {
    fn default() -> Self {
        Self {
            query: Object::new("Query"),
            mutation: None,
            subscription: None,
            types: Vec::new(),
            context: Arc::new(GraphqlContext::default()),
            workers: GraphqlWorkerExecutor::default(),
            subscription_capacity: 16,
            max_depth: None,
            max_complexity: None,
            introspection: true,
        }
    }
}

fn resolver_input(ctx: &ResolverContext<'_>, field_name: &str, default_context: &Arc<GraphqlContext>) -> ResolverInput {
    let arguments = serde_json::to_value(ctx.args.as_index_map()).unwrap_or_else(|_| json!({}));
    let parent = ctx.parent_value.try_to_value()
        .ok()
        .and_then(|value| serde_json::to_value(value).ok())
        .unwrap_or(serde_json::Value::Null);
    let context = ctx.data_opt::<Arc<GraphqlContext>>().cloned().unwrap_or_else(|| default_context.clone());
    ResolverInput { field_name: field_name.to_owned(), arguments, parent, context }
}

fn dynamic_field(field: GraphqlField, workers: GraphqlWorkerExecutor, context: Arc<GraphqlContext>) -> Field {
    let GraphqlField { name, ty, arguments, resolver } = field;
    let resolver_name = name.clone();
    let mut dynamic = Field::new(name, ty, move |ctx| {
        let input = resolver_input(&ctx, &resolver_name, &context);
        let path = ctx.path_node.map(path_from_node);
        let tracker = ctx.data_opt::<Arc<ErrorPathStore>>().cloned();
        let future = workers.dispatch(resolver.clone(), input);
        FieldFuture::new(async move {
            match future.await {
                Ok(value) => Ok(value),
                Err(error) => {
                    if let Some(tracker) = tracker {
                        tracker.record(error.message.clone(), path.unwrap_or_default()).await;
                    }
                    Err(error)
                }
            }
        })
    });
    for argument in arguments { dynamic = dynamic.argument(argument); }
    dynamic
}

fn dynamic_subscription_field(field: GraphqlSubscriptionField, workers: GraphqlWorkerExecutor, context: Arc<GraphqlContext>) -> SubscriptionField {
    let GraphqlSubscriptionField { name, ty, arguments, resolver } = field;
    let resolver_name = name.clone();
    let mut dynamic = SubscriptionField::new(name, ty, move |ctx| {
        let input = resolver_input(&ctx, &resolver_name, &context);
        let future = workers.dispatch_subscription(resolver.clone(), input);
        SubscriptionFieldFuture::new(future)
    });
    for argument in arguments { dynamic = dynamic.argument(argument); }
    dynamic
}

/// An owned, cheap-to-clone schema and execution adapter.
#[derive(Clone)]
pub struct GraphqlSchema {
    schema: Schema,
    context: Arc<GraphqlContext>,
    workers: GraphqlWorkerExecutor,
    subscription_capacity: usize,
}

impl fmt::Debug for GraphqlSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("GraphqlSchema").finish_non_exhaustive() }
}

impl GraphqlSchema {
    pub fn builder() -> GraphqlSchemaBuilder { GraphqlSchemaBuilder::default() }
    pub fn sdl(&self) -> String { self.schema.sdl() }

    pub async fn execute(&self, request: impl Into<GraphqlRequest>) -> GraphqlResponse {
        let request = request.into();
        let tracker = Arc::new(ErrorPathStore::default());
        let mut response = self.schema.execute(request.into_request(self.context.clone(), tracker.clone())).await;
        tracker.apply(&mut response).await;
        GraphqlResponse(response)
    }

    pub async fn execute_query(&self, query: impl Into<String>) -> GraphqlResponse {
        self.execute(GraphqlRequest::new(query)).await
    }

    pub fn subscribe(&self, request: impl Into<GraphqlRequest>) -> GraphqlSubscription {
        let request = request.into();
        let tracker = Arc::new(ErrorPathStore::default());
        let stream = self.schema.execute_stream(request.into_request(self.context.clone(), tracker.clone()));
        GraphqlSubscription::start(stream, self.subscription_capacity, tracker)
    }

    pub async fn execute_http(&self, request: HttpRequest<Vec<u8>>) -> HttpResponse<Vec<u8>> {
        let parsed = match parse_http_request(&request) {
            Ok(request) => request,
            Err(error) => return graphql_http_error(error),
        };
        let response = self.execute(parsed).await;
        let mut http_response = HttpResponse::new(response.json_bytes());
        *http_response.status_mut() = StatusCode::OK;
        http_response.headers_mut().insert(http::header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
        http_response
    }


    pub fn inner(&self) -> &Schema { &self.schema }
    pub fn worker_executor(&self) -> &GraphqlWorkerExecutor { &self.workers }
}

/// GraphQL request, matching the standard JSON-over-HTTP shape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GraphqlRequest {
    pub query: String,
    #[serde(default, rename = "operationName")]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub variables: serde_json::Value,
    #[serde(default)]
    pub extensions: Option<serde_json::Value>,
    #[serde(skip)]
    pub context: Option<Arc<GraphqlContext>>,
}

impl GraphqlRequest {
    pub fn new(query: impl Into<String>) -> Self { Self { query: query.into(), variables: json!({}), ..Self::default() } }
    #[must_use]
    pub fn operation_name(mut self, name: impl Into<String>) -> Self { self.operation_name = Some(name.into()); self }
    #[must_use]
    pub fn variables(mut self, variables: serde_json::Value) -> Self { self.variables = variables; self }
    #[must_use]
    pub fn context(mut self, context: Arc<GraphqlContext>) -> Self { self.context = Some(context); self }

    fn into_request(self, default_context: Arc<GraphqlContext>, tracker: Arc<ErrorPathStore>) -> Request {
        let mut request = Request::new(self.query).variables(Variables::from_json(self.variables));
        if let Some(name) = self.operation_name { request = request.operation_name(name); }
        request.data(self.context.unwrap_or(default_context)).data(tracker)
    }
}

impl From<&str> for GraphqlRequest { fn from(query: &str) -> Self { Self::new(query) } }
impl From<String> for GraphqlRequest { fn from(query: String) -> Self { Self::new(query) } }

/// Owned GraphQL response preserving async-graphql's data, errors, locations, and paths.
pub struct GraphqlResponse(Response);

impl fmt::Debug for GraphqlResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
}

impl GraphqlResponse {
    pub fn is_ok(&self) -> bool { self.0.is_ok() }
    pub fn data(&self) -> &Value { &self.0.data }
    pub fn errors(&self) -> &[ServerError] { &self.0.errors }
    pub fn into_inner(self) -> Response { self.0 }
    pub fn json_value(&self) -> serde_json::Value { serde_json::to_value(&self.0).unwrap_or_else(|_| json!({"errors":[{"message":"response serialization failed"}]})) }
    pub fn json_bytes(&self) -> Vec<u8> { serde_json::to_vec(&self.0).unwrap_or_else(|_| b"{\"errors\":[{\"message\":\"response serialization failed\"}]}".to_vec()) }
}

/// Bounded subscription stream handle. Dropping it cancels its producer.
pub struct GraphqlSubscription {
    receiver: Arc<Mutex<mpsc::Receiver<GraphqlResponse>>>,
    cancellation: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
    wake: Arc<Notify>,
    queued: Arc<AtomicUsize>,
    capacity: usize,
}

impl fmt::Debug for GraphqlSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraphqlSubscription").field("capacity", &self.capacity).field("queued", &self.queued.load(Ordering::Acquire)).finish()
    }
}

impl GraphqlSubscription {
    fn start(mut stream: BoxStream<'static, Response>, capacity: usize, tracker: Arc<ErrorPathStore>) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        let cancellation = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(Notify::new());
        let queued = Arc::new(AtomicUsize::new(0));
        let producer_cancel = cancellation.clone();
        let producer_completed = completed.clone();
        let producer_wake = wake.clone();
        let producer_queued = queued.clone();
        tokio::spawn(async move {
            loop {
                let mut response = tokio::select! {
                    _ = producer_wake.notified() => break,
                    response = stream.next() => match response { Some(response) => response, None => break },
                };
                if producer_cancel.load(Ordering::Acquire) { break; }
                tracker.apply(&mut response).await;
                producer_queued.fetch_add(1, Ordering::AcqRel);
                if sender.send(GraphqlResponse(response)).await.is_err() {
                    producer_queued.fetch_sub(1, Ordering::AcqRel);
                    break;
                }
            }
            producer_completed.store(true, Ordering::Release);
        });
        Self { receiver: Arc::new(Mutex::new(receiver)), cancellation, completed, wake, queued, capacity }
    }
    /// Receive the next event, or `None` after completion/cancellation.
    pub async fn next(&self) -> Option<GraphqlResponse> {
        if self.cancellation.load(Ordering::Acquire) { return None; }
        let cancellation = self.wake.notified();
        let mut receiver = self.receiver.lock().await;
        let result = tokio::select! {
            _ = cancellation => None,
            result = receiver.recv() => result,
        };
        if result.is_some() { self.queued.fetch_sub(1, Ordering::AcqRel); }
        result
    }

    /// True when no event is currently buffered and the producer may be pending.
    pub fn pending(&self) -> bool {
        self.queued.load(Ordering::Acquire) == 0
            && !self.cancellation.load(Ordering::Acquire)
            && !self.completed.load(Ordering::Acquire)
    }
    pub fn capacity(&self) -> usize { self.capacity }
    pub fn is_cancelled(&self) -> bool { self.cancellation.load(Ordering::Acquire) }
    pub fn cancel(&self) { if !self.cancellation.swap(true, Ordering::AcqRel) { self.wake.notify_waiters(); } }
}

impl Drop for GraphqlSubscription { fn drop(&mut self) { self.cancel(); } }

/// Parse an HTTP GraphQL request according to the standard GET/POST shapes.
pub fn parse_http_request(request: &HttpRequest<Vec<u8>>) -> Result<GraphqlRequest, GraphqlHttpError> {
    match *request.method() {
        Method::GET => {
            let query = request.uri().query().ok_or_else(|| GraphqlHttpError::InvalidQuery("missing query parameter".into()))?;
            parse_get_query(query)
        }
        Method::POST => {
            let content_type = request.headers().get(http::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").trim();
            if content_type != "application/json" && content_type != "application/graphql+json" { return Err(GraphqlHttpError::UnsupportedMediaType); }
            serde_json::from_slice(request.body()).map_err(|err| GraphqlHttpError::InvalidBody(err.to_string()))
        }
        _ => Err(GraphqlHttpError::MethodNotAllowed),
    }
}

#[derive(Debug)]
pub enum GraphqlHttpError { MethodNotAllowed, UnsupportedMediaType, InvalidBody(String), InvalidQuery(String) }
impl fmt::Display for GraphqlHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MethodNotAllowed => f.write_str("GraphQL only supports GET and POST"),
            Self::UnsupportedMediaType => f.write_str("GraphQL POST requires application/json or application/graphql+json"),
            Self::InvalidBody(message) => write!(f, "invalid GraphQL JSON body: {message}"),
            Self::InvalidQuery(message) => write!(f, "invalid GraphQL query parameters: {message}"),
        }
    }
}
impl std::error::Error for GraphqlHttpError {}

fn parse_get_query(query: &str) -> Result<GraphqlRequest, GraphqlHttpError> {
    let mut values = HashMap::new();
    for part in query.split('&').filter(|part| !part.is_empty()) {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        values.insert(percent_decode(key)?, percent_decode(value)?);
    }
    let source = values.remove("query").ok_or_else(|| GraphqlHttpError::InvalidQuery("missing query parameter".into()))?;
    let mut request = GraphqlRequest::new(source);
    request.operation_name = values.remove("operationName");
    if let Some(variables) = values.remove("variables") {
        request.variables = serde_json::from_str(&variables).map_err(|err| GraphqlHttpError::InvalidQuery(format!("variables must be JSON: {err}")))?;
    }
    Ok(request)
}

fn percent_decode(value: &str) -> Result<String, GraphqlHttpError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => output.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let high = hex(bytes[index + 1]).ok_or_else(|| GraphqlHttpError::InvalidQuery("invalid percent escape".into()))?;
                let low = hex(bytes[index + 2]).ok_or_else(|| GraphqlHttpError::InvalidQuery("invalid percent escape".into()))?;
                output.push((high << 4) | low); index += 2;
            }
            b'%' => return Err(GraphqlHttpError::InvalidQuery("truncated percent escape".into())),
            byte => output.push(byte),
        }
        index += 1;
    }
    String::from_utf8(output).map_err(|_| GraphqlHttpError::InvalidQuery("query parameters must be UTF-8".into()))
}

fn hex(byte: u8) -> Option<u8> { match byte { b'0'..=b'9' => Some(byte - b'0'), b'a'..=b'f' => Some(byte - b'a' + 10), b'A'..=b'F' => Some(byte - b'A' + 10), _ => None } }

fn graphql_http_error(error: GraphqlHttpError) -> HttpResponse<Vec<u8>> {
    let status = match error { GraphqlHttpError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED, GraphqlHttpError::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE, _ => StatusCode::BAD_REQUEST };
    let body = serde_json::to_vec(&json!({"errors":[{"message": error.to_string()}]})).unwrap_or_else(|_| b"{\"errors\":[{\"message\":\"bad request\"}]}".to_vec());
    let mut response = HttpResponse::new(body);
    *response.status_mut() = status;
    response.headers_mut().insert(http::header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    if status == StatusCode::METHOD_NOT_ALLOWED { response.headers_mut().insert(http::header::ALLOW, HeaderValue::from_static("GET, POST")); }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_graphql::dynamic::{Field, FieldFuture, Object, TypeRef};
    use futures_util::stream;
    use std::{sync::atomic::AtomicU64, thread, time::Duration};

    fn json_value(value: serde_json::Value) -> Value { Value::from_json(value).unwrap() }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nested_alias_fragments_directives_and_variables() {
        let user = Object::new("User").field(Field::new("name", TypeRef::named_nn("String"), |_| FieldFuture::from_value(Some(Value::from("Ada")))));
        let echo = resolver_callback(|input: ResolverInput| async move {
            Ok(Some(json_value(input.arguments.get("prefix").cloned().unwrap_or_else(|| json!("missing")))))
        });
        let schema = GraphqlSchema::builder()
            .register_object(user)
            .field(GraphqlRoot::Query, GraphqlField::new("user", TypeRef::named_nn("User"), static_value_resolver(json_value(json!({"name": "Ada"})))))
            .field(GraphqlRoot::Query, GraphqlField::new("enabled", TypeRef::named_nn("Boolean"), static_value_resolver(Value::from(true))))
            .field(GraphqlRoot::Query, GraphqlField::new("echo", TypeRef::named_nn("String"), echo).argument(InputValue::new("prefix", TypeRef::named("String")).default_value("default")))
            .finish().unwrap();
        let response = schema.execute(GraphqlRequest::new(
            "query Demo($show: Boolean! = true) { alias: user { ...UserFields } enabled @include(if: $show) skipped: enabled @skip(if: false) echo } fragment UserFields on User { name }",
        ).variables(json!({"show": true})).operation_name("Demo")).await;
        assert!(response.is_ok());
        assert_eq!(response.json_value()["data"]["alias"]["name"], "Ada");
        assert_eq!(response.json_value()["data"]["enabled"], true);
        assert_eq!(response.json_value()["data"]["skipped"], true);
        assert_eq!(response.json_value()["data"]["echo"], "default");

    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mutation_fields_are_serial_and_callbacks_run_on_workers() {
        let caller = thread::current().id();
        let worker_ok = Arc::new(AtomicBool::new(true));
        let order = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let field = |name: &'static str, worker_ok: Arc<AtomicBool>, order: Arc<tokio::sync::Mutex<Vec<&'static str>>>| {
            let callback = resolver_callback(move |_| {
                let worker_ok = worker_ok.clone();
                let order = order.clone();
                Box::pin(async move {
                    worker_ok.store(thread::current().id() != caller, Ordering::Relaxed);
                    order.lock().await.push(name);
                    Ok(Some(Value::from(name)))
                })
            });
            GraphqlField::new(name, TypeRef::named_nn("String"), callback)
        };
        let schema = GraphqlSchema::builder()
            .field(GraphqlRoot::Query, GraphqlField::new("health", TypeRef::named_nn("Boolean"), static_value_resolver(Value::from(true))))
            .field(GraphqlRoot::Mutation, field("first", worker_ok.clone(), order.clone()))
            .field(GraphqlRoot::Mutation, field("second", worker_ok.clone(), order.clone()))
            .finish().unwrap();
        let response = schema.execute_query("mutation { first second }").await;
        assert!(response.is_ok());
        assert_eq!(*order.lock().await, vec!["first", "second"]);
        assert!(worker_ok.load(Ordering::Relaxed));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn max_depth_rejects_over_deep_queries() {
        let user = || Object::new("User").field(Field::new("name", TypeRef::named_nn("String"), |_| FieldFuture::from_value(Some(Value::from("Ada")))));
        let build = || {
            GraphqlSchema::builder()
                .register_object(user())
                .field(GraphqlRoot::Query, GraphqlField::new("user", TypeRef::named_nn("User"), static_value_resolver(json_value(json!({"name": "Ada"})))))
        };
        let open = build().finish().unwrap();
        let nested = open.execute_query("{ user { name } }").await;
        assert!(nested.is_ok());
        assert_eq!(nested.json_value()["data"]["user"]["name"], "Ada");
        let guarded = build().max_depth(Some(1)).finish().unwrap();
        let rejected = guarded.execute_query("{ user { name } }").await;
        assert!(!rejected.is_ok());
        assert!(!rejected.errors().is_empty());
        let roomy = build().max_depth(Some(10)).finish().unwrap();
        assert!(roomy.execute_query("{ user { name } }").await.is_ok());
        let cleared = build().max_depth(Some(1)).max_depth(None).finish().unwrap();
        assert!(cleared.execute_query("{ user { name } }").await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn introspection_toggle_hides_schema() {
        let build = || {
            GraphqlSchema::builder().field(
                GraphqlRoot::Query,
                GraphqlField::new("ok", TypeRef::named_nn("Boolean"), static_value_resolver(Value::from(true))),
            )
        };
        let open = build().finish().unwrap();
        let visible = open.execute_query("{ __schema { queryType { name } } }").await;
        assert_eq!(visible.json_value()["data"]["__schema"]["queryType"]["name"], "Query");
        let closed = build().introspection(false).finish().unwrap();
        let hidden = closed.execute_query("{ __schema { queryType { name } } }").await;
        assert!(!hidden.is_ok());
        assert!(!hidden.errors().is_empty());
        assert!(closed.execute_query("{ ok }").await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn introspection_and_validation_errors_preserve_locations_and_paths() {
        let schema = GraphqlSchema::builder()
            .field(GraphqlRoot::Query, GraphqlField::new("bad", TypeRef::named_nn("String"), resolver_callback(|_| async { Err(Error::new("boom")) })))
            .finish().unwrap();
        let introspection = schema.execute_query("{ __schema { queryType { name } } }").await;
        assert_eq!(introspection.json_value()["data"]["__schema"]["queryType"]["name"], "Query");
        let response = schema.execute_query("{ bad }").await;
        assert_eq!(response.errors()[0].path.len(), 1);
        assert!(matches!(&response.errors()[0].path[0], PathSegment::Field(name) if name == "bad"));
        assert!(!response.errors()[0].locations.is_empty());
        let invalid = schema.execute_query("{ missing }").await;
        assert!(!invalid.is_ok());
        assert!(!invalid.errors()[0].locations.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscription_is_bounded_and_cancellable() {
        let produced = Arc::new(AtomicU64::new(0));
        let produced_for_callback = produced.clone();
        let subscription = subscription_callback(move |_| {
            let produced = produced_for_callback.clone();
            async move {
                let values = (0..3).map(move |index| {
                    produced.fetch_add(1, Ordering::Relaxed);
                    Ok(Value::from(index))
                });
                Ok(stream::iter(values))
            }
        });
        let schema = GraphqlSchema::builder().subscription_capacity(1)
            .field(GraphqlRoot::Query, GraphqlField::new("health", TypeRef::named_nn("Boolean"), static_value_resolver(Value::from(true))))
            .subscription_field(GraphqlSubscriptionField::new("events", TypeRef::named_nn("Int"), subscription))
            .finish().unwrap();
        let events = schema.subscribe("subscription { events }");
        assert_eq!(events.capacity(), 1);
        let first = events.next().await.unwrap();
        assert_eq!(first.json_value()["data"]["events"], 0);
        let second = events.next().await.unwrap();
        assert_eq!(second.json_value()["data"]["events"], 1);
        events.cancel();
        assert!(events.is_cancelled());
        assert!(tokio::time::timeout(Duration::from_millis(100), events.next()).await.unwrap().is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_get_post_and_method_content_type_errors() {
        let schema = GraphqlSchema::builder().field(GraphqlRoot::Query, GraphqlField::new("ok", TypeRef::named_nn("Boolean"), static_value_resolver(Value::from(true)))).finish().unwrap();
        let request = HttpRequest::builder().method(Method::GET).uri("/?query=%7Bok%7D").body(Vec::new()).unwrap();
        assert_eq!(schema.execute_http(request).await.status(), StatusCode::OK);
        let request = HttpRequest::builder().method(Method::POST).uri("/").header(http::header::CONTENT_TYPE, "application/json").body(br#"{"query":"{ ok }"}"#.to_vec()).unwrap();
        assert_eq!(schema.execute_http(request).await.status(), StatusCode::OK);
        let request = HttpRequest::builder().method(Method::PUT).uri("/").body(Vec::new()).unwrap();
        assert_eq!(schema.execute_http(request).await.status(), StatusCode::METHOD_NOT_ALLOWED);
        let request = HttpRequest::builder().method(Method::POST).uri("/").header(http::header::CONTENT_TYPE, "text/plain").body(b"{ ok }".to_vec()).unwrap();
        assert_eq!(schema.execute_http(request).await.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
}
