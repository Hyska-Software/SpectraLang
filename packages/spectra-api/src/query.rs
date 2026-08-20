use crate::handles::ApiHandleTable;
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Mutex, OnceLock};

pub const QUERY_TYPE_STRING: SpectraHostValue = 1;
pub const QUERY_TYPE_INT: SpectraHostValue = 2;
pub const QUERY_TYPE_BOOL: SpectraHostValue = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pairs: Vec<(String, String)>,
    values: BTreeMap<String, Vec<String>>,
}

impl Query {
    pub fn parse(input: &str) -> Result<Self, QueryParseError> {
        parse_query(input)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn values(&self, key: &str) -> &[String] {
        self.values.get(key).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn first(&self, key: &str) -> Option<&str> {
        self.values(key).first().map(String::as_str)
    }

    pub fn get(&self, key: &str, index: usize) -> Option<&str> {
        self.values(key).get(index).map(String::as_str)
    }

    pub fn int(&self, key: &str, index: usize) -> Result<i64, QueryBindError> {
        let Some(value) = self.get(key, index) else {
            return Err(QueryBindError::MissingField(key.to_string()));
        };
        coerce_int(key, value)
    }

    pub fn bool(&self, key: &str, index: usize) -> Result<bool, QueryBindError> {
        let Some(value) = self.get(key, index) else {
            return Err(QueryBindError::MissingField(key.to_string()));
        };
        coerce_bool(key, value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryParseErrorKind {
    InvalidPercentEncoding,
    InvalidUtf8,
    EmptyKey,
    ControlCharacter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryParseError {
    pub kind: QueryParseErrorKind,
    pub position: usize,
    pub message: String,
}

impl QueryParseError {
    fn new(kind: QueryParseErrorKind, position: usize, message: impl Into<String>) -> Self {
        Self {
            kind,
            position,
            message: message.into(),
        }
    }

    fn code(&self) -> SpectraHostValue {
        match self.kind {
            QueryParseErrorKind::InvalidPercentEncoding => 1,
            QueryParseErrorKind::InvalidUtf8 => 2,
            QueryParseErrorKind::EmptyKey => 3,
            QueryParseErrorKind::ControlCharacter => 4,
        }
    }
}

impl fmt::Display for QueryParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.position)
    }
}

impl std::error::Error for QueryParseError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryValueType {
    String,
    Int,
    Bool,
}

impl QueryValueType {
    fn from_code(code: SpectraHostValue) -> Option<Self> {
        match code {
            QUERY_TYPE_STRING => Some(Self::String),
            QUERY_TYPE_INT => Some(Self::Int),
            QUERY_TYPE_BOOL => Some(Self::Bool),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryFieldSchema {
    pub name: String,
    pub value_type: QueryValueType,
    pub required: bool,
    pub repeated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuerySchema {
    fields: Vec<QueryFieldSchema>,
}

impl QuerySchema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(
        mut self,
        name: impl Into<String>,
        value_type: QueryValueType,
        required: bool,
        repeated: bool,
    ) -> Result<Self, QueryBindError> {
        let name = name.into();
        if name.is_empty() {
            return Err(QueryBindError::InvalidSchema(
                "query schema field name cannot be empty".to_string(),
            ));
        }
        if self.fields.iter().any(|field| field.name == name) {
            return Err(QueryBindError::InvalidSchema(format!(
                "duplicate query schema field {name:?}"
            )));
        }
        self.fields.push(QueryFieldSchema {
            name,
            value_type,
            required,
            repeated,
        });
        Ok(self)
    }

    pub fn bind(&self, query: &Query) -> QueryBinding {
        let mut values = HashMap::new();
        for field in &self.fields {
            let raw_values = query.values(&field.name);
            if raw_values.is_empty() {
                if field.required {
                    return QueryBinding::error(QueryBindError::MissingField(field.name.clone()));
                }
                values.insert(field.name.clone(), Vec::new());
                continue;
            }
            if !field.repeated && raw_values.len() > 1 {
                return QueryBinding::error(QueryBindError::RepeatedScalarField(
                    field.name.clone(),
                ));
            }
            for value in raw_values {
                if let Err(error) = validate_coercion(field, value) {
                    return QueryBinding::error(error);
                }
            }
            values.insert(field.name.clone(), raw_values.to_vec());
        }
        QueryBinding {
            ok: true,
            values,
            error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryBindError {
    InvalidSchema(String),
    MissingField(String),
    RepeatedScalarField(String),
    InvalidInt { field: String, value: String },
    InvalidBool { field: String, value: String },
}

impl QueryBindError {
    fn code(&self) -> SpectraHostValue {
        match self {
            Self::InvalidSchema(_) => 10,
            Self::MissingField(_) => 11,
            Self::RepeatedScalarField(_) => 12,
            Self::InvalidInt { .. } => 13,
            Self::InvalidBool { .. } => 14,
        }
    }
}

impl fmt::Display for QueryBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSchema(message) => write!(f, "invalid query schema: {message}"),
            Self::MissingField(field) => write!(f, "missing required query field {field:?}"),
            Self::RepeatedScalarField(field) => {
                write!(
                    f,
                    "query field {field:?} is scalar but appeared more than once"
                )
            }
            Self::InvalidInt { field, value } => {
                write!(f, "query field {field:?} expected int, got {value:?}")
            }
            Self::InvalidBool { field, value } => {
                write!(f, "query field {field:?} expected bool, got {value:?}")
            }
        }
    }
}

impl std::error::Error for QueryBindError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryBinding {
    ok: bool,
    values: HashMap<String, Vec<String>>,
    error: Option<QueryBindError>,
}

impl QueryBinding {
    fn error(error: QueryBindError) -> Self {
        Self {
            ok: false,
            values: HashMap::new(),
            error: Some(error),
        }
    }

    pub fn ok(&self) -> bool {
        self.ok
    }

    pub fn error_message(&self) -> String {
        self.error
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    pub fn count(&self, field: &str) -> usize {
        self.values.get(field).map(Vec::len).unwrap_or(0)
    }

    pub fn get(&self, field: &str, index: usize) -> Option<&str> {
        self.values
            .get(field)
            .and_then(|values| values.get(index))
            .map(String::as_str)
    }

    pub fn int(&self, field: &str, index: usize) -> Result<i64, QueryBindError> {
        let Some(value) = self.get(field, index) else {
            return Err(QueryBindError::MissingField(field.to_string()));
        };
        coerce_int(field, value)
    }

    pub fn bool(&self, field: &str, index: usize) -> Result<bool, QueryBindError> {
        let Some(value) = self.get(field, index) else {
            return Err(QueryBindError::MissingField(field.to_string()));
        };
        coerce_bool(field, value)
    }
}

struct QueryStore {
    queries: ApiHandleTable<Query>,
    schemas: ApiHandleTable<QuerySchema>,
    bindings: ApiHandleTable<QueryBinding>,
    last_error_code: SpectraHostValue,
    last_error_message: String,
}

impl QueryStore {
    fn new() -> Self {
        Self {
            queries: ApiHandleTable::new(HandleKind::ApiQuery),
            schemas: ApiHandleTable::new(HandleKind::ApiQuerySchema),
            bindings: ApiHandleTable::new(HandleKind::ApiQueryBinding),
            last_error_code: 0,
            last_error_message: String::new(),
        }
    }

    fn query_handle(&mut self, query: Query) -> SpectraHostValue {
        self.queries.insert(query)
    }

    fn schema_handle(&mut self, schema: QuerySchema) -> SpectraHostValue {
        self.schemas.insert(schema)
    }

    fn binding_handle(&mut self, binding: QueryBinding) -> SpectraHostValue {
        self.bindings.insert(binding)
    }

    fn clear_error(&mut self) {
        self.last_error_code = 0;
        self.last_error_message.clear();
    }

    fn set_parse_error(&mut self, error: QueryParseError) {
        self.last_error_code = error.code();
        self.last_error_message = error.to_string();
    }

    fn set_bind_error(&mut self, error: QueryBindError) {
        self.last_error_code = error.code();
        self.last_error_message = error.to_string();
    }
}

fn store() -> &'static Mutex<QueryStore> {
    static STORE: OnceLock<Mutex<QueryStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(QueryStore::new()))
}

fn parse_query(input: &str) -> Result<Query, QueryParseError> {
    let query = isolate_query(input);
    if query.is_empty() {
        return Ok(Query {
            pairs: Vec::new(),
            values: BTreeMap::new(),
        });
    }
    let mut pairs = Vec::new();
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pair_start = 0usize;
    for raw_pair in query.split('&') {
        if raw_pair.is_empty() {
            pair_start += 1;
            continue;
        }
        let equal_offset = raw_pair.find('=');
        let (raw_key, raw_value) = match equal_offset {
            Some(offset) => (&raw_pair[..offset], &raw_pair[offset + 1..]),
            None => (raw_pair, ""),
        };
        let key = percent_decode(raw_key, pair_start)?;
        if key.is_empty() {
            return Err(QueryParseError::new(
                QueryParseErrorKind::EmptyKey,
                pair_start,
                "query key cannot be empty",
            ));
        }
        let value_start = equal_offset
            .map(|offset| pair_start + offset + 1)
            .unwrap_or(pair_start);
        let value = percent_decode(raw_value, value_start)?;
        pairs.push((key.clone(), value.clone()));
        values.entry(key).or_default().push(value);
        pair_start += raw_pair.len() + 1;
    }
    Ok(Query { pairs, values })
}

fn isolate_query(input: &str) -> &str {
    let after_question = input
        .find('?')
        .map(|idx| &input[idx + 1..])
        .unwrap_or_else(|| input.strip_prefix('?').unwrap_or(input));
    after_question
        .find('#')
        .map(|idx| &after_question[..idx])
        .unwrap_or(after_question)
}

fn percent_decode(input: &str, base_position: usize) -> Result<String, QueryParseError> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(QueryParseError::new(
                        QueryParseErrorKind::InvalidPercentEncoding,
                        base_position + index,
                        "percent encoding must contain two hex digits",
                    ));
                }
                let hi = hex_value(bytes[index + 1]);
                let lo = hex_value(bytes[index + 2]);
                let (Some(hi), Some(lo)) = (hi, lo) else {
                    return Err(QueryParseError::new(
                        QueryParseErrorKind::InvalidPercentEncoding,
                        base_position + index,
                        "percent encoding contains a non-hex digit",
                    ));
                };
                decoded.push((hi << 4) | lo);
                index += 3;
            }
            byte if byte < 0x20 || byte == 0x7f => {
                return Err(QueryParseError::new(
                    QueryParseErrorKind::ControlCharacter,
                    base_position + index,
                    "query strings cannot contain raw control characters",
                ));
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| {
        QueryParseError::new(
            QueryParseErrorKind::InvalidUtf8,
            base_position,
            "percent decoded query component is not valid UTF-8",
        )
    })
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn validate_coercion(field: &QueryFieldSchema, value: &str) -> Result<(), QueryBindError> {
    match field.value_type {
        QueryValueType::String => Ok(()),
        QueryValueType::Int => coerce_int(&field.name, value).map(|_| ()),
        QueryValueType::Bool => coerce_bool(&field.name, value).map(|_| ()),
    }
}

fn coerce_int(field: &str, value: &str) -> Result<i64, QueryBindError> {
    value
        .parse::<i64>()
        .map_err(|_| QueryBindError::InvalidInt {
            field: field.to_string(),
            value: value.to_string(),
        })
}

fn coerce_bool(field: &str, value: &str) -> Result<bool, QueryBindError> {
    match value {
        "true" | "1" | "on" | "yes" => Ok(true),
        "false" | "0" | "off" | "no" => Ok(false),
        _ => Err(QueryBindError::InvalidBool {
            field: field.to_string(),
            value: value.to_string(),
        }),
    }
}

pub extern "C" fn type_string(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, QUERY_TYPE_STRING)
}

pub extern "C" fn type_int(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, QUERY_TYPE_INT)
}

pub extern "C" fn type_bool(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, QUERY_TYPE_BOOL)
}

pub extern "C" fn parse(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(input) = read_spectra_string(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("query store poisoned");
    match Query::parse(&input) {
        Ok(query) => {
            store.clear_error();
            let handle = store.query_handle(query);
            write_result(ctx, handle)
        }
        Err(error) => {
            store.set_parse_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("query store poisoned");
    let len = store.queries.get(&args[0]).map(Query::len).unwrap_or(0);
    write_result(ctx, len as SpectraHostValue)
}

pub extern "C" fn has(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("query store poisoned");
    let has = store
        .queries
        .get(&args[0])
        .map(|query| query.has(&key))
        .unwrap_or(false);
    write_result(ctx, i64::from(has))
}

pub extern "C" fn count(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("query store poisoned");
    let count = store
        .queries
        .get(&args[0])
        .map(|query| query.values(&key).len())
        .unwrap_or(0);
    write_result(ctx, count as SpectraHostValue)
}

pub extern "C" fn first(ctx: *mut SpectraHostCallContext) -> i32 {
    query_string_lookup(ctx, |query, key| query.first(key).map(str::to_string))
}

pub extern "C" fn value(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[2].max(0) as usize;
    let store = store().lock().expect("query store poisoned");
    let value = store
        .queries
        .get(&args[0])
        .and_then(|query| query.get(&key, index))
        .unwrap_or("");
    write_result(ctx, alloc_spectra_string(value))
}

pub extern "C" fn int(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[2].max(0) as usize;
    let mut store = store().lock().expect("query store poisoned");
    let result = store
        .queries
        .get(&args[0])
        .ok_or_else(|| QueryBindError::MissingField(key.clone()))
        .and_then(|query| query.int(&key, index));
    match result {
        Ok(value) => {
            store.clear_error();
            write_result(ctx, value)
        }
        Err(error) => {
            store.set_bind_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn bool(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[2].max(0) as usize;
    let mut store = store().lock().expect("query store poisoned");
    let result = store
        .queries
        .get(&args[0])
        .ok_or_else(|| QueryBindError::MissingField(key.clone()))
        .and_then(|query| query.bool(&key, index));
    match result {
        Ok(value) => {
            store.clear_error();
            write_result(ctx, i64::from(value))
        }
        Err(error) => {
            store.set_bind_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn schema(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store().lock().expect("query store poisoned");
    store.clear_error();
    let handle = store.schema_handle(QuerySchema::new());
    write_result(ctx, handle)
}

pub extern "C" fn schema_field(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(value_type) = QueryValueType::from_code(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let required = args[3] != 0;
    let repeated = args[4] != 0;
    let mut store = store().lock().expect("query store poisoned");
    let Some(existing) = store.schemas.get(&args[0]).cloned() else {
        return write_result(ctx, 0);
    };
    match existing.with_field(name, value_type, required, repeated) {
        Ok(schema) => {
            store.clear_error();
            let handle = store.schema_handle(schema);
            write_result(ctx, handle)
        }
        Err(error) => {
            store.set_bind_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn bind(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("query store poisoned");
    let binding = {
        let Some(query) = store.queries.get(&args[0]) else {
            let error = QueryBindError::MissingField("<query>".to_string());
            store.set_bind_error(error.clone());
            let handle = store.binding_handle(QueryBinding::error(error));
            return write_result(ctx, handle);
        };
        let Some(schema) = store.schemas.get(&args[1]) else {
            let error = QueryBindError::InvalidSchema("schema handle not found".to_string());
            store.set_bind_error(error.clone());
            let handle = store.binding_handle(QueryBinding::error(error));
            return write_result(ctx, handle);
        };
        schema.bind(query)
    };
    if let Some(error) = binding.error.clone() {
        store.set_bind_error(error);
    } else {
        store.clear_error();
    }
    let handle = store.binding_handle(binding);
    write_result(ctx, handle)
}

pub extern "C" fn binding_ok(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("query store poisoned");
    let ok = store
        .bindings
        .get(&args[0])
        .map(QueryBinding::ok)
        .unwrap_or(false);
    write_result(ctx, i64::from(ok))
}

pub extern "C" fn binding_error(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("query store poisoned");
    let message = store
        .bindings
        .get(&args[0])
        .map(QueryBinding::error_message)
        .unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&message))
}

pub extern "C" fn binding_count(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(field) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("query store poisoned");
    let count = store
        .bindings
        .get(&args[0])
        .map(|binding| binding.count(&field))
        .unwrap_or(0);
    write_result(ctx, count as SpectraHostValue)
}

pub extern "C" fn binding_value(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(field) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[2].max(0) as usize;
    let store = store().lock().expect("query store poisoned");
    let value = store
        .bindings
        .get(&args[0])
        .and_then(|binding| binding.get(&field, index))
        .unwrap_or("");
    write_result(ctx, alloc_spectra_string(value))
}

pub extern "C" fn binding_int(ctx: *mut SpectraHostCallContext) -> i32 {
    binding_int_lookup(ctx, |binding, field, index| binding.int(field, index))
}

pub extern "C" fn binding_bool(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(field) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[2].max(0) as usize;
    let mut store = store().lock().expect("query store poisoned");
    let result = store
        .bindings
        .get(&args[0])
        .ok_or_else(|| QueryBindError::MissingField(field.clone()))
        .and_then(|binding| binding.bool(&field, index));
    match result {
        Ok(value) => {
            store.clear_error();
            write_result(ctx, i64::from(value))
        }
        Err(error) => {
            store.set_bind_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let store = store().lock().expect("query store poisoned");
    write_result(ctx, store.last_error_code)
}

pub extern "C" fn error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let store = store().lock().expect("query store poisoned");
    write_result(ctx, alloc_spectra_string(&store.last_error_message))
}

fn query_string_lookup(
    ctx: *mut SpectraHostCallContext,
    lookup: impl FnOnce(&Query, &str) -> Option<String>,
) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("query store poisoned");
    let value = store
        .queries
        .get(&args[0])
        .and_then(|query| lookup(query, &key))
        .unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&value))
}

fn binding_int_lookup(
    ctx: *mut SpectraHostCallContext,
    lookup: impl FnOnce(&QueryBinding, &str, usize) -> Result<i64, QueryBindError>,
) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(field) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[2].max(0) as usize;
    let mut store = store().lock().expect("query store poisoned");
    let result = store
        .bindings
        .get(&args[0])
        .ok_or_else(|| QueryBindError::MissingField(field.clone()))
        .and_then(|binding| lookup(binding, &field, index));
    match result {
        Ok(value) => {
            store.clear_error();
            write_result(ctx, value)
        }
        Err(error) => {
            store.set_bind_error(error);
            write_result(ctx, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_repeated_and_reserved_query_values() {
        let query = Query::parse("/search?q=rust%20lang&tag=api&tag=web&plus=a+b&empty=")
            .expect("query parses");
        assert_eq!(query.len(), 4);
        assert_eq!(query.first("q"), Some("rust lang"));
        assert_eq!(query.values("tag"), ["api".to_string(), "web".to_string()]);
        assert_eq!(query.first("plus"), Some("a+b"));
        assert_eq!(query.first("empty"), Some(""));
        assert!(query.has("q"));
    }

    #[test]
    fn rejects_malformed_percent_encoding_and_control_chars() {
        let bad_hex = Query::parse("name=%GG").expect_err("bad percent rejected");
        assert_eq!(bad_hex.kind, QueryParseErrorKind::InvalidPercentEncoding);
        let short = Query::parse("name=%A").expect_err("short percent rejected");
        assert_eq!(short.kind, QueryParseErrorKind::InvalidPercentEncoding);
        let control = Query::parse("name=bad\nvalue").expect_err("control rejected");
        assert_eq!(control.kind, QueryParseErrorKind::ControlCharacter);
    }

    #[test]
    fn binds_typed_schema_and_reports_type_errors() {
        let query = Query::parse("page=2&published=true&tag=rust&tag=api").expect("query");
        let schema = QuerySchema::new()
            .with_field("page", QueryValueType::Int, true, false)
            .expect("page")
            .with_field("published", QueryValueType::Bool, true, false)
            .expect("published")
            .with_field("tag", QueryValueType::String, false, true)
            .expect("tag");
        let binding = schema.bind(&query);
        assert!(binding.ok());
        assert_eq!(binding.int("page", 0), Ok(2));
        assert_eq!(binding.bool("published", 0), Ok(true));
        assert_eq!(binding.count("tag"), 2);
        assert_eq!(binding.get("tag", 1), Some("api"));

        let mismatch = Query::parse("page=two").expect("mismatch query");
        let failed = schema.bind(&mismatch);
        assert!(!failed.ok());
        assert!(failed.error_message().contains("expected int"));
    }
}
