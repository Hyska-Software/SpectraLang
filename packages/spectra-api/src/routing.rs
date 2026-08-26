use crate::handles::ApiHandleTable;
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::{HandleId, HandleKind, HandleTable};
use serde_json::{json, Map, Value};
use std::fmt;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub const ROUTE_METHOD_GET: SpectraHostValue = 1;
pub const ROUTE_METHOD_HEAD: SpectraHostValue = 2;
pub const ROUTE_METHOD_POST: SpectraHostValue = 3;
pub const ROUTE_METHOD_PUT: SpectraHostValue = 4;
pub const ROUTE_METHOD_PATCH: SpectraHostValue = 5;
pub const ROUTE_METHOD_DELETE: SpectraHostValue = 6;
pub const ROUTE_METHOD_OPTIONS: SpectraHostValue = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RouteMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

impl RouteMethod {
    pub fn from_code(code: SpectraHostValue) -> Option<Self> {
        match code {
            ROUTE_METHOD_GET => Some(Self::Get),
            ROUTE_METHOD_HEAD => Some(Self::Head),
            ROUTE_METHOD_POST => Some(Self::Post),
            ROUTE_METHOD_PUT => Some(Self::Put),
            ROUTE_METHOD_PATCH => Some(Self::Patch),
            ROUTE_METHOD_DELETE => Some(Self::Delete),
            ROUTE_METHOD_OPTIONS => Some(Self::Options),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteSegment {
    Literal(String),
    Param {
        name: String,
        constraint: Option<String>,
    },
    Wildcard {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub id: SpectraHostValue,
    pub method: RouteMethod,
    pub pattern: String,
    pub segments: Vec<RouteSegment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteMatch {
    pub route_id: SpectraHostValue,
    pub params: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteConflict {
    pub existing: String,
    pub candidate: String,
    pub message: String,
}

impl fmt::Display for RouteConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "route conflict between '{}' and '{}': {}",
            self.existing, self.candidate, self.message
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteError {
    InvalidPattern(String),
    InvalidPath(String),
    Conflict(RouteConflict),
    InvalidSchema(String),
    InvalidStatusCode(i64),
    InvalidServers(String),
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPattern(pattern) => write!(f, "invalid route pattern {pattern:?}"),
            Self::InvalidPath(path) => write!(f, "invalid request path {path:?}"),
            Self::Conflict(conflict) => write!(f, "{conflict}"),
            Self::InvalidSchema(detail) => write!(f, "invalid JSON request schema: {detail}"),
            Self::InvalidStatusCode(code) => {
                write!(f, "invalid HTTP status code {code}: expected 100..=599")
            }
            Self::InvalidServers(detail) => {
                write!(f, "invalid OpenAPI servers list: {detail}")
            }
        }
    }
}

impl std::error::Error for RouteError {}

#[derive(Clone, Debug, Default)]
struct RouteNode {
    literals: HashMap<String, RouteNode>,
    param: Option<ParamEdge>,
    wildcard: Option<WildcardEdge>,
    handlers: HashMap<RouteMethod, SpectraHostValue>,
}

#[derive(Clone, Debug)]
struct ParamEdge {
    name: String,
    constraint: Option<String>,
    node: Box<RouteNode>,
}

#[derive(Clone, Debug)]
struct WildcardEdge {
    name: String,
    route_by_method: HashMap<RouteMethod, SpectraHostValue>,
}
#[derive(Clone, Debug, Default)]
pub struct Router {
    routes: HashMap<SpectraHostValue, Route>,
    root: RouteNode,
    request_schemas: HashMap<(RouteMethod, String), Value>,
    response_schemas: HashMap<(RouteMethod, String), BTreeMap<u16, Value>>,
    servers: Vec<String>,
}

impl Router {
    pub fn add(
        &mut self,
        method: RouteMethod,
        pattern: impl Into<String>,
    ) -> Result<SpectraHostValue, RouteError> {
        let pattern = pattern.into();
        let segments = parse_pattern(&pattern)?;
        detect_conflict(&self.root, &self.routes, method, &segments, &pattern, 0)?;
        let id = next_route_id(method);
        insert_route(&mut self.root, method, &segments, id, 0);
        self.routes.insert(
            id,
            Route {
                id,
                method,
                pattern,
                segments,
            },
        );
        Ok(id)
    }

    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    pub fn match_path(
        &self,
        method: RouteMethod,
        path: impl AsRef<str>,
    ) -> Result<Option<RouteMatch>, RouteError> {
        let raw = path.as_ref();
        let segments = parse_path(raw)?;
        let mut params = HashMap::new();
        Ok(match_node(&self.root, method, &segments, 0, &mut params)
            .map(|route_id| RouteMatch { route_id, params }))
    }

    /// Store a JSON request-body schema for the route registered under
    /// `method` + `path_template`. The template may be written either exactly
    /// as registered (`/users/{id:int}`) or in plain OpenAPI form
    /// (`/users/{id}`); it is resolved against registered routes by parsed
    /// segments. Returns `Ok(false)` when no registered route matches.
    pub fn set_request_schema(
        &mut self,
        method: RouteMethod,
        path_template: &str,
        json_schema: &str,
    ) -> Result<bool, RouteError> {
        let schema: Value = serde_json::from_str(json_schema)
            .map_err(|error| RouteError::InvalidSchema(error.to_string()))?;
        let segments = parse_pattern(path_template)?;
        let matches_template = |route: &Route| {
            route.method == method
                && route.segments.len() == segments.len()
                && route.segments.iter().zip(&segments).all(|(a, b)| match (a, b) {
                    // Constraints are ignored so both `/users/{id:int}` and the
                    // plain OpenAPI form `/users/{id}` resolve to the route.
                    (RouteSegment::Param { name: a, .. }, RouteSegment::Param { name: b, .. }) => {
                        a == b
                    }
                    (RouteSegment::Wildcard { name: a }, RouteSegment::Wildcard { name: b }) => {
                        a == b
                    }
                    (RouteSegment::Literal(a), RouteSegment::Literal(b)) => a == b,
                    _ => false,
                })
        };
        let pattern = self
            .routes
            .values()
            .find(|route| matches_template(route))
            .map(|route| route.pattern.clone());
        match pattern {
            Some(pattern) => {
                self.request_schemas.insert((method, pattern), schema);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn set_response_schema(
        &mut self,
        method: RouteMethod,
        path_template: &str,
        status: i64,
        json_schema: &str,
    ) -> Result<bool, RouteError> {
        if !(100..=599).contains(&status) {
            return Err(RouteError::InvalidStatusCode(status));
        }
        let schema: Value = serde_json::from_str(json_schema)
            .map_err(|e| RouteError::InvalidSchema(e.to_string()))?;
        let segments = parse_pattern(path_template)?;
        let matches = |route: &Route| {
            route.method == method
                && route.segments.len() == segments.len()
                && route.segments.iter().zip(&segments).all(|(a, b)| match (a, b) {
                    (RouteSegment::Param { name: a, .. }, RouteSegment::Param { name: b, .. }) => a == b,
                    (RouteSegment::Wildcard { name: a }, RouteSegment::Wildcard { name: b }) => a == b,
                    (RouteSegment::Literal(a), RouteSegment::Literal(b)) => a == b,
                    _ => false,
                })
        };
        let pattern = self.routes.values().find(|r| matches(r)).map(|r| r.pattern.clone());
        match pattern {
            Some(p) => {
                self.response_schemas.entry((method, p)).or_default().insert(status as u16, schema);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn set_servers(&mut self, servers_json: &str) -> Result<(), RouteError> {
        let value: Value = serde_json::from_str(servers_json)
            .map_err(|e| RouteError::InvalidServers(e.to_string()))?;
        let arr = value.as_array().ok_or_else(|| RouteError::InvalidServers("servers must be a JSON array".to_string()))?;
        let mut servers = Vec::new();
        for v in arr {
            let s = v.as_str().ok_or_else(|| RouteError::InvalidServers("each server must be a string URL".to_string()))?;
            if s.trim().is_empty() {
                return Err(RouteError::InvalidServers("server URL must not be empty".to_string()));
            }
            servers.push(s.to_string());
        }
        self.servers = servers;
        Ok(())
    }
}

fn route_handles() -> &'static Mutex<HandleTable<RouteMethod>> {
    static ROUTES: OnceLock<Mutex<HandleTable<RouteMethod>>> = OnceLock::new();
    ROUTES.get_or_init(|| Mutex::new(HandleTable::new(HandleKind::ApiRoute)))
}

fn next_route_id(method: RouteMethod) -> SpectraHostValue {
    route_handles()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(method)
        .raw()
}

struct RouterStore {
    routers: ApiHandleTable<Router>,
    matches: ApiHandleTable<RouteMatch>,
    last_conflict: String,
}

impl RouterStore {
    fn new() -> Self {
        Self {
            routers: ApiHandleTable::new(HandleKind::ApiRouter),
            matches: ApiHandleTable::new(HandleKind::ApiRouteMatch),
            last_conflict: String::new(),
        }
    }

    fn router_handle(&mut self) -> SpectraHostValue {
        self.routers.insert(Router::default())
    }

    fn match_handle(&mut self, route_match: RouteMatch) -> SpectraHostValue {
        self.matches.insert(route_match)
    }
}

fn store() -> &'static Mutex<RouterStore> {
    static STORE: OnceLock<Mutex<RouterStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(RouterStore::new()))
}

pub(crate) fn clone_router(handle: SpectraHostValue) -> Option<Router> {
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    store.routers.get(&handle).cloned()
}

pub(crate) fn route_method(route_id: SpectraHostValue) -> Option<RouteMethod> {
    if let Ok(handle) = HandleId::from_raw(route_id) {
        if handle.kind() == HandleKind::ApiRoute {
            if let Some(method) = route_handles()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(handle)
                .ok()
                .copied()
            {
                return Some(method);
            }
        }
    }
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let method = store
        .routers
        .iter()
        .find_map(|(_, router)| router.routes.get(&route_id).map(|route| route.method));
    method
}

pub fn parse_pattern(pattern: &str) -> Result<Vec<RouteSegment>, RouteError> {
    if !is_valid_route_path_like(pattern) {
        return Err(RouteError::InvalidPattern(pattern.to_string()));
    }
    let mut segments = Vec::new();
    for segment in split_segments(pattern) {
        if segment.starts_with('{') || segment.ends_with('}') {
            let Some(inner) = segment
                .strip_prefix('{')
                .and_then(|value| value.strip_suffix('}'))
            else {
                return Err(RouteError::InvalidPattern(pattern.to_string()));
            };
            let (name, constraint) = match inner.split_once(':') {
                Some((name, constraint)) => (name, Some(constraint.to_string())),
                None => (inner, None),
            };
            if !is_identifier(name) || constraint.as_deref().is_some_and(str::is_empty) {
                return Err(RouteError::InvalidPattern(pattern.to_string()));
            }
            segments.push(RouteSegment::Param {
                name: name.to_string(),
                constraint,
            });
        } else if let Some(name) = segment.strip_prefix('*') {
            if !is_identifier(name) {
                return Err(RouteError::InvalidPattern(pattern.to_string()));
            }
            segments.push(RouteSegment::Wildcard {
                name: name.to_string(),
            });
            if segments.len() != split_segments(pattern).count() {
                return Err(RouteError::InvalidPattern(pattern.to_string()));
            }
        } else {
            segments.push(RouteSegment::Literal(segment.to_string()));
        }
    }
    Ok(segments)
}

fn parse_path(path: &str) -> Result<Vec<String>, RouteError> {
    if !is_valid_route_path_like(path) {
        return Err(RouteError::InvalidPath(path.to_string()));
    }
    Ok(split_segments(path).map(str::to_string).collect())
}

fn split_segments(path: &str) -> impl Iterator<Item = &str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
}

fn is_valid_route_path_like(path: &str) -> bool {
    !path.is_empty() && path.starts_with('/') && !path.bytes().any(|b| b <= b' ' || b == 0x7f)
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn detect_conflict(
    node: &RouteNode,
    routes: &HashMap<SpectraHostValue, Route>,
    method: RouteMethod,
    segments: &[RouteSegment],
    candidate: &str,
    index: usize,
) -> Result<(), RouteError> {
    if index == segments.len() {
        if let Some(existing_id) = node.handlers.get(&method) {
            return Err(conflict(routes, *existing_id, candidate, "duplicate route"));
        }
        if let Some(wildcard) = &node.wildcard {
            if let Some(existing_id) = wildcard.route_by_method.get(&method) {
                return Err(conflict(
                    routes,
                    *existing_id,
                    candidate,
                    "wildcard route already captures this path",
                ));
            }
        }
        return Ok(());
    }

    match &segments[index] {
        RouteSegment::Literal(literal) => {
            if let Some(param) = &node.param {
                if subtree_has_method(&param.node, method) {
                    return Err(conflict(
                        routes,
                        first_route_for_method(&param.node, method).unwrap_or_default(),
                        candidate,
                        "literal route overlaps an existing parameter route",
                    ));
                }
            }
            if let Some(wildcard) = &node.wildcard {
                if let Some(existing_id) = wildcard.route_by_method.get(&method) {
                    return Err(conflict(
                        routes,
                        *existing_id,
                        candidate,
                        "literal route overlaps an existing wildcard route",
                    ));
                }
            }
            if let Some(child) = node.literals.get(literal) {
                detect_conflict(child, routes, method, segments, candidate, index + 1)?;
            }
        }
        RouteSegment::Param { constraint, .. } => {
            for literal_child in node.literals.values() {
                if subtree_has_method(literal_child, method) {
                    return Err(conflict(
                        routes,
                        first_route_for_method(literal_child, method).unwrap_or_default(),
                        candidate,
                        "parameter route overlaps an existing literal route",
                    ));
                }
            }
            if let Some(existing) = &node.param {
                if constraints_overlap(existing.constraint.as_deref(), constraint.as_deref())
                    && subtree_has_method(&existing.node, method)
                {
                    return Err(conflict(
                        routes,
                        first_route_for_method(&existing.node, method).unwrap_or_default(),
                        candidate,
                        "parameter route overlaps an existing parameter route",
                    ));
                }
                detect_conflict(
                    &existing.node,
                    routes,
                    method,
                    segments,
                    candidate,
                    index + 1,
                )?;
            }
            if let Some(wildcard) = &node.wildcard {
                if let Some(existing_id) = wildcard.route_by_method.get(&method) {
                    return Err(conflict(
                        routes,
                        *existing_id,
                        candidate,
                        "parameter route overlaps an existing wildcard route",
                    ));
                }
            }
        }
        RouteSegment::Wildcard { .. } => {
            if subtree_has_method(node, method) {
                return Err(conflict(
                    routes,
                    first_route_for_method(node, method).unwrap_or_default(),
                    candidate,
                    "wildcard route overlaps an existing route",
                ));
            }
        }
    }

    Ok(())
}

fn conflict(
    routes: &HashMap<SpectraHostValue, Route>,
    route_id: SpectraHostValue,
    candidate: &str,
    message: &str,
) -> RouteError {
    let existing = routes
        .get(&route_id)
        .map(|route| route.pattern.clone())
        .unwrap_or_else(|| format!("#{route_id}"));
    RouteError::Conflict(RouteConflict {
        existing,
        candidate: candidate.to_string(),
        message: message.to_string(),
    })
}

fn subtree_has_method(node: &RouteNode, method: RouteMethod) -> bool {
    node.handlers.contains_key(&method)
        || node
            .literals
            .values()
            .any(|child| subtree_has_method(child, method))
        || node
            .param
            .as_ref()
            .is_some_and(|edge| subtree_has_method(&edge.node, method))
        || node
            .wildcard
            .as_ref()
            .is_some_and(|edge| edge.route_by_method.contains_key(&method))
}

fn first_route_for_method(node: &RouteNode, method: RouteMethod) -> Option<SpectraHostValue> {
    node.handlers
        .get(&method)
        .copied()
        .or_else(|| {
            node.literals
                .values()
                .find_map(|child| first_route_for_method(child, method))
        })
        .or_else(|| {
            node.param
                .as_ref()
                .and_then(|edge| first_route_for_method(&edge.node, method))
        })
        .or_else(|| {
            node.wildcard
                .as_ref()
                .and_then(|edge| edge.route_by_method.get(&method).copied())
        })
}

fn constraints_overlap(left: Option<&str>, right: Option<&str>) -> bool {
    left.is_none() || right.is_none() || left == right
}

fn insert_route(
    node: &mut RouteNode,
    method: RouteMethod,
    segments: &[RouteSegment],
    route_id: SpectraHostValue,
    index: usize,
) {
    if index == segments.len() {
        node.handlers.insert(method, route_id);
        return;
    }
    match &segments[index] {
        RouteSegment::Literal(literal) => {
            let child = node.literals.entry(literal.clone()).or_default();
            insert_route(child, method, segments, route_id, index + 1);
        }
        RouteSegment::Param { name, constraint } => {
            let edge = node.param.get_or_insert_with(|| ParamEdge {
                name: name.clone(),
                constraint: constraint.clone(),
                node: Box::<RouteNode>::default(),
            });
            insert_route(&mut edge.node, method, segments, route_id, index + 1);
        }
        RouteSegment::Wildcard { name } => {
            let edge = node.wildcard.get_or_insert_with(|| WildcardEdge {
                name: name.clone(),
                route_by_method: HashMap::new(),
            });
            edge.route_by_method.insert(method, route_id);
        }
    }
}

fn match_node(
    node: &RouteNode,
    method: RouteMethod,
    path: &[String],
    index: usize,
    params: &mut HashMap<String, String>,
) -> Option<SpectraHostValue> {
    if index == path.len() {
        return node.handlers.get(&method).copied();
    }

    let segment = &path[index];
    if let Some(child) = node.literals.get(segment) {
        if let Some(route) = match_node(child, method, path, index + 1, params) {
            return Some(route);
        }
    }

    if let Some(edge) = &node.param {
        if edge
            .constraint
            .as_deref()
            .is_none_or(|constraint| regex_like_matches(constraint, segment))
        {
            params.insert(edge.name.clone(), segment.clone());
            if let Some(route) = match_node(&edge.node, method, path, index + 1, params) {
                return Some(route);
            }
            params.remove(&edge.name);
        }
    }

    if let Some(edge) = &node.wildcard {
        if let Some(route) = edge.route_by_method.get(&method).copied() {
            params.insert(edge.name.clone(), path[index..].join("/"));
            return Some(route);
        }
    }

    None
}

fn regex_like_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern
        .strip_prefix('^')
        .unwrap_or(pattern)
        .strip_suffix('$')
        .unwrap_or(pattern);
    if pattern == r"\d+" || pattern == "[0-9]+" {
        return !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    }
    if pattern == r"\w+" {
        return !value.is_empty()
            && value
                .bytes()
                .all(|b| b == b'_' || b.is_ascii_alphanumeric());
    }
    if pattern == "[a-zA-Z]+" {
        return !value.is_empty() && value.bytes().all(|b| b.is_ascii_alphabetic());
    }
    if pattern == ".+" {
        return !value.is_empty();
    }
    pattern == value
}

fn add_route_host(ctx: *mut SpectraHostCallContext, method: RouteMethod) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(pattern) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = store.routers.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match router.add(method, pattern) {
        Ok(route_id) => write_result(ctx, route_id),
        Err(RouteError::Conflict(conflict)) => {
            store.last_conflict = conflict.to_string();
            write_result(ctx, 0)
        }
        Err(error) => {
            store.last_conflict = error.to_string();
            HOST_STATUS_INVALID_ARGUMENT
        }
    }
}

pub extern "C" fn router_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let handle = store.router_handle();
    write_result(ctx, handle)
}

pub extern "C" fn route_count(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = store.routers.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, router.route_count() as SpectraHostValue)
}

pub extern "C" fn route_id(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, args[0].max(0))
}

pub extern "C" fn route_add(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(method) = RouteMethod::from_code(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(pattern) = read_spectra_string(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = store.routers.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match router.add(method, pattern) {
        Ok(route_id) => write_result(ctx, route_id),
        Err(RouteError::Conflict(conflict)) => {
            store.last_conflict = conflict.to_string();
            write_result(ctx, 0)
        }
        Err(error) => {
            store.last_conflict = error.to_string();
            HOST_STATUS_INVALID_ARGUMENT
        }
    }
}

pub extern "C" fn get(ctx: *mut SpectraHostCallContext) -> i32 {
    add_route_host(ctx, RouteMethod::Get)
}

pub extern "C" fn post(ctx: *mut SpectraHostCallContext) -> i32 {
    add_route_host(ctx, RouteMethod::Post)
}

pub extern "C" fn put(ctx: *mut SpectraHostCallContext) -> i32 {
    add_route_host(ctx, RouteMethod::Put)
}

pub extern "C" fn patch(ctx: *mut SpectraHostCallContext) -> i32 {
    add_route_host(ctx, RouteMethod::Patch)
}

pub extern "C" fn delete(ctx: *mut SpectraHostCallContext) -> i32 {
    add_route_host(ctx, RouteMethod::Delete)
}

pub extern "C" fn route_match(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(method) = RouteMethod::from_code(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(path) = read_spectra_string(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = store.routers.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match router.match_path(method, path) {
        Ok(Some(route_match)) => {
            let handle = store.match_handle(route_match);
            write_result(ctx, handle)
        }
        Ok(None) => write_result(ctx, 0),
        Err(error) => {
            store.last_conflict = error.to_string();
            HOST_STATUS_INVALID_ARGUMENT
        }
    }
}

pub extern "C" fn match_route_id(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    if args[0] == 0 {
        return write_result(ctx, 0);
    }
    let Some(route_match) = store.matches.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, route_match.route_id)
}

pub extern "C" fn match_param(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    if args[0] == 0 {
        return write_result(ctx, alloc_spectra_string(""));
    }
    let Some(route_match) = store.matches.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        alloc_spectra_string(
            route_match
                .params
                .get(&name)
                .map(String::as_str)
                .unwrap_or(""),
        ),
    )
}

pub extern "C" fn match_param_int(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    if args[0] == 0 {
        return write_result(ctx, -1);
    }
    let Some(route_match) = store.matches.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let value = route_match
        .params
        .get(&name)
        .and_then(|value| value.parse::<SpectraHostValue>().ok())
        .unwrap_or(-1);
    write_result(ctx, value)
}

pub extern "C" fn last_conflict(ctx: *mut SpectraHostCallContext) -> i32 {
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, alloc_spectra_string(&store.last_conflict))
}

/// OpenAPI 3.1 schema for a path-parameter constraint. Constraints are the
/// regex-like patterns understood by the trie (`regex_like_matches`); the
/// reserved keywords `int`/`float`/`string` map to schema types, numeric
/// patterns map to `integer`, and anything else passes through honestly as a
/// string with its `pattern`.
fn openapi_param_schema(constraint: Option<&str>) -> Value {
    match constraint {
        None | Some("") => json!({ "type": "string" }),
        Some("int") => json!({ "type": "integer" }),
        Some("float") | Some("number") => json!({ "type": "number" }),
        Some("string") => json!({ "type": "string" }),
        Some(pattern) => {
            let mut schema = Map::new();
            let is_integer = matches!(pattern, r"\d+" | "[0-9]+");
            schema.insert(
                "type".to_string(),
                Value::String(if is_integer { "integer" } else { "string" }.to_string()),
            );
            schema.insert("pattern".to_string(), json!(pattern));
            Value::Object(schema)
        }
    }
}

fn openapi_path_template(segments: &[RouteSegment]) -> String {
    let mut template = String::new();
    for segment in segments {
        template.push('/');
        match segment {
            RouteSegment::Literal(literal) => template.push_str(literal),
            RouteSegment::Param { name, .. } | RouteSegment::Wildcard { name } => {
                template.push('{');
                template.push_str(name);
                template.push('}');
            }
        }
    }
    if template.is_empty() {
        template.push('/');
    }
    template
}

fn openapi_method_name(method: RouteMethod) -> &'static str {
    match method {
        RouteMethod::Get => "get",
        RouteMethod::Head => "head",
        RouteMethod::Post => "post",
        RouteMethod::Put => "put",
        RouteMethod::Patch => "patch",
        RouteMethod::Delete => "delete",
        RouteMethod::Options => "options",
    }
}

const OPENAPI_METHOD_ORDER: [(RouteMethod, usize); 7] = [
    (RouteMethod::Get, 0),
    (RouteMethod::Head, 1),
    (RouteMethod::Post, 2),
    (RouteMethod::Put, 3),
    (RouteMethod::Patch, 4),
    (RouteMethod::Delete, 5),
    (RouteMethod::Options, 6),
];

/// Export a router as a minimal but valid OpenAPI 3.1 JSON document: one path
/// entry per registered route pattern, one operation per method, typed path
/// parameters derived from the trie constraints, a default `200` response,
/// and — when a schema hint was stored via `Router::set_request_schema` — a
/// `requestBody` whose `application/json` schema is the hint embedded
/// verbatim. Routes without a hint omit `requestBody`.
pub fn export_openapi(router: &Router, title: &str, version: &str) -> String {
    // Group routes by their OpenAPI path template; keep deterministic order.
    let mut grouped: BTreeMap<String, Vec<(&Route, usize)>> = BTreeMap::new();
    for route in router.routes.values() {
        let template = openapi_path_template(&route.segments);
        let order = OPENAPI_METHOD_ORDER
            .iter()
            .find(|(method, _)| *method == route.method)
            .map(|(_, index)| *index)
            .unwrap_or(usize::MAX);
        grouped.entry(template).or_default().push((route, order));
    }

    let mut paths = Map::new();
    for (template, mut routes) in grouped {
        routes.sort_by_key(|(_, order)| *order);
        let mut item = Map::new();
        for (route, _) in routes {
            let mut parameters = Vec::new();
            for segment in &route.segments {
                let (name, constraint, description) = match segment {
                    RouteSegment::Param { name, constraint } => (
                        name,
                        constraint.as_deref(),
                        None,
                    ),
                    RouteSegment::Wildcard { name } => (
                        name,
                        None,
                        Some("matches the remaining path tail"),
                    ),
                    RouteSegment::Literal(_) => continue,
                };
                let mut parameter = Map::new();
                parameter.insert("name".to_string(), json!(name));
                parameter.insert("in".to_string(), json!("path"));
                parameter.insert("required".to_string(), json!(true));
                if let Some(description) = description {
                    parameter.insert("description".to_string(), json!(description));
                }
                parameter.insert("schema".to_string(), openapi_param_schema(constraint));
                parameters.push(Value::Object(parameter));
            }
            let mut responses = Map::new();
            responses.insert("200".to_string(), json!({ "description": "OK" }));
            if let Some(schemas) = router.response_schemas.get(&(route.method, route.pattern.clone())) {
                for (status, schema) in schemas {
                    responses.insert(status.to_string(), json!({
                        "description": "",
                        "content": { "application/json": { "schema": schema } }
                    }));
                }
            }
            let mut operation = json!({
                "parameters": parameters,
                "responses": Value::Object(responses),
            });
            if let Some(schema) = router
                .request_schemas
                .get(&(route.method, route.pattern.clone()))
            {
                operation["requestBody"] = json!({
                    "content": {
                        "application/json": { "schema": schema },
                    },
                });
            }
            item.insert(
                openapi_method_name(route.method).to_string(),
                operation,
            );
        }
        paths.insert(template, Value::Object(item));
    }

    let mut spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": title,
            "version": version,
        },
        "paths": Value::Object(paths),
    });
    if !router.servers.is_empty() {
        spec["servers"] = Value::Array(router.servers.iter().map(|url| json!({ "url": url })).collect());
    }
    serde_json::to_string(&spec).expect("OpenAPI spec serializes")
}

pub extern "C" fn routes_export_openapi(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(title) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(version) = read_spectra_string(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = store.routers.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        alloc_spectra_string(&export_openapi(router, &title, &version)),
    )
}

/// `routing.routes_set_request_schema(router, method, path_template, schema) -> bool`.
/// Stores a JSON request-body schema hint for a registered route. An invalid
/// JSON schema is a typed error (`RouteError::InvalidSchema`) surfaced as
/// `HOST_STATUS_INVALID_ARGUMENT` with the message in `last_conflict`.
pub extern "C" fn routes_set_request_schema(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 4) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(method) = RouteMethod::from_code(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(path_template) = read_spectra_string(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(schema) = read_spectra_string(args[3]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = store.routers.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match router.set_request_schema(method, &path_template, &schema) {
        Ok(stored) => write_result(ctx, SpectraHostValue::from(stored)),
        Err(error) => {
            store.last_conflict = error.to_string();
            HOST_STATUS_INVALID_ARGUMENT
        }
    }
}

pub fn benchmark_100k_lookup() -> Result<u128, RouteError> {
    let mut router = Router::default();
    for idx in 0..100_000 {
        router.add(RouteMethod::Get, format!("/bench/{idx}/item"))?;
    }
    let start = Instant::now();
    let found = router.match_path(RouteMethod::Get, "/bench/99999/item")?;
    let elapsed = start.elapsed().as_micros();
    if found.is_none() {
        return Err(RouteError::InvalidPath("/bench/99999/item".to_string()));
    }
    Ok(elapsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_literals_params_wildcards_and_regex_constraints() {
        let mut router = Router::default();
        let users = router.add(RouteMethod::Get, "/users").expect("literal");
        let user = router.add(RouteMethod::Post, "/users/{id}").expect("param");
        let files = router
            .add(RouteMethod::Get, "/files/*path")
            .expect("wildcard");
        let order = router
            .add(RouteMethod::Get, r"/orders/{id:\d+}")
            .expect("regex");
        assert_eq!(
            spectra_runtime::handles::HandleId::from_raw(users)
                .expect("route handle")
                .kind(),
            HandleKind::ApiRoute
        );

        assert_eq!(
            router
                .match_path(RouteMethod::Get, "/users")
                .unwrap()
                .unwrap()
                .route_id,
            users
        );
        let matched_user = router
            .match_path(RouteMethod::Post, "/users/42")
            .unwrap()
            .unwrap();
        assert_eq!(matched_user.route_id, user);
        assert_eq!(
            matched_user.params.get("id").map(String::as_str),
            Some("42")
        );

        let matched_file = router
            .match_path(RouteMethod::Get, "/files/a/b/c.txt")
            .unwrap()
            .unwrap();
        assert_eq!(matched_file.route_id, files);
        assert_eq!(
            matched_file.params.get("path").map(String::as_str),
            Some("a/b/c.txt")
        );

        let matched_order = router
            .match_path(RouteMethod::Get, "/orders/123")
            .unwrap()
            .unwrap();
        assert_eq!(matched_order.route_id, order);
        assert!(router
            .match_path(RouteMethod::Get, "/orders/not-digits")
            .unwrap()
            .is_none());
    }

    #[test]
    fn reports_literal_parameter_and_wildcard_conflicts() {
        let mut router = Router::default();
        router.add(RouteMethod::Get, "/users/{id}").expect("param");
        let conflict = router
            .add(RouteMethod::Get, "/users/me")
            .expect_err("literal should conflict with param");
        assert!(conflict.to_string().contains("/users/me"));

        let mut wildcard_router = Router::default();
        wildcard_router
            .add(RouteMethod::Get, "/files/*path")
            .expect("wildcard");
        let conflict = wildcard_router
            .add(RouteMethod::Get, "/files/public/logo.png")
            .expect_err("wildcard should conflict");
        assert!(conflict.to_string().contains("/files/public/logo.png"));
    }

    #[test]
    fn host_store_exposes_route_match_params_and_conflict_text() {
        let mut router = Router::default();
        let route = router
            .add(RouteMethod::Get, r"/orders/{id:\d+}")
            .expect("route");
        let matched = router
            .match_path(RouteMethod::Get, "/orders/42")
            .unwrap()
            .expect("match");
        assert_eq!(matched.route_id, route);
        assert_eq!(matched.params.get("id").map(String::as_str), Some("42"));
    }

    #[test]
    fn route_handles_are_unique_across_router_instances() {
        let mut first = Router::default();
        let mut second = Router::default();
        let first_route = first.add(RouteMethod::Get, "/first").unwrap();
        let second_route = second.add(RouteMethod::Get, "/second").unwrap();
        assert_ne!(first_route, second_route);
        assert_eq!(
            spectra_runtime::handles::HandleId::from_raw(second_route)
                .expect("route handle")
                .kind(),
            HandleKind::ApiRoute
        );
    }

    #[test]
    fn one_hundred_thousand_routes_lookup_is_sub_millisecond() {
        let elapsed = benchmark_100k_lookup().expect("benchmark");
        assert!(
            elapsed < 1_000,
            "100k route lookup took {elapsed}us, expected <1000us"
        );
    }

    #[test]
    fn export_openapi_emits_paths_methods_typed_params_and_default_response() {
        let mut router = Router::default();
        router
            .add(RouteMethod::Get, "/users/{id:int}")
            .expect("get users");
        router
            .add(RouteMethod::Post, "/users/{id:int}")
            .expect("post users");
        router
            .add(RouteMethod::Get, r"/orders/{ref:\d+}")
            .expect("regex constraint");
        router
            .add(RouteMethod::Get, "/files/*path")
            .expect("wildcard");

        let spec = export_openapi(&router, "Demo API", "0.1.0");
        let value: Value = serde_json::from_str(&spec).expect("spec parses as JSON");

        assert_eq!(value["openapi"], "3.1.0");
        assert_eq!(value["info"]["title"], "Demo API");
        assert_eq!(value["info"]["version"], "0.1.0");

        let users = &value["paths"]["/users/{id}"];
        assert!(users["get"].is_object(), "GET /users/{{id}} exported");
        assert!(users["post"].is_object(), "POST /users/{{id}} exported");
        let id_param = &users["get"]["parameters"][0];
        assert_eq!(id_param["name"], "id");
        assert_eq!(id_param["in"], "path");
        assert_eq!(id_param["required"], true);
        assert_eq!(id_param["schema"]["type"], "integer");
        assert_eq!(
            users["get"]["responses"]["200"]["description"],
            "OK",
            "default 200 response"
        );

        // Regex-like numeric constraints from the trie export as integers.
        let order_param = &value["paths"]["/orders/{ref}"]["get"]["parameters"][0];
        assert_eq!(order_param["schema"]["type"], "integer");
        assert_eq!(order_param["schema"]["pattern"], r"\d+");

        // Wildcards become string path parameters covering the tail.
        let tail_param = &value["paths"]["/files/{path}"]["get"]["parameters"][0];
        assert_eq!(tail_param["name"], "path");
        assert_eq!(tail_param["schema"]["type"], "string");
    }

    #[test]
    fn export_openapi_is_deterministic_and_empty_router_yields_empty_paths() {
        let router = Router::default();
        let first = export_openapi(&router, "A", "1");
        let second = export_openapi(&router, "A", "1");
        assert_eq!(first, second);
        let value: Value = serde_json::from_str(&first).expect("parses");
        assert_eq!(value["paths"].as_object().map(Map::len), Some(0));
    }
    #[test]
    fn set_request_schema_resolves_route_and_embeds_verbatim_in_export() {
        let mut router = Router::default();
        router
            .add(RouteMethod::Post, "/users/{id:int}")
            .expect("post users");

        // Unregistered route reports false without storing anything.
        assert!(!router
            .set_request_schema(RouteMethod::Post, "/orders", "{}")
            .expect("unregistered route is not an error"));

        let schema = r#"{"type":"object","properties":{"id":{"type":"integer"}}}"#;
        // OpenAPI-style template resolves to the registered constrained route.
        assert!(router
            .set_request_schema(RouteMethod::Post, "/users/{id}", schema)
            .expect("registered route accepts hint"));

        let invalid = router.set_request_schema(RouteMethod::Post, "/users/{id:int}", "{nope");
        assert!(
            matches!(invalid, Err(RouteError::InvalidSchema(_))),
            "invalid JSON is a typed InvalidSchema error, got {invalid:?}"
        );

        let value: Value =
            serde_json::from_str(&export_openapi(&router, "T", "0")).expect("spec parses");
        assert_eq!(
            value["paths"]["/users/{id}"]["post"]["requestBody"]["content"]["application/json"]
                ["schema"],
            serde_json::from_str::<Value>(schema).expect("schema is valid JSON")
        );
    }
}

