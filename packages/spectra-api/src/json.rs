use crate::handles::ApiHandleTable;
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use serde_json::{Map, Number, Value};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_SUCCESS,
};
use std::fmt;
use std::str::FromStr;
use std::sync::{LazyLock, Mutex};

pub const JSON_KIND_INVALID: SpectraHostValue = 0;
pub const JSON_KIND_NULL: SpectraHostValue = 1;
pub const JSON_KIND_BOOL: SpectraHostValue = 2;
pub const JSON_KIND_NUMBER: SpectraHostValue = 3;
pub const JSON_KIND_STRING: SpectraHostValue = 4;
pub const JSON_KIND_ARRAY: SpectraHostValue = 5;
pub const JSON_KIND_OBJECT: SpectraHostValue = 6;

/// Object keys preserve document insertion order: parsing `{"b":1,"a":2}`
/// round-trips in the authored order and updating an existing key never moves
/// its position.
///
/// Decision: [`serde_json`] is compiled with its `preserve_order` feature so
/// the parser keeps document key order (without it, serde's `Map` sorts keys
/// and the original order is lost before this code ever sees it), while
/// [`JsonObject`] is a thin Vec-backed insertion-ordered map of our own —
/// `serde_json::Map` itself only supports `serde_json::Value` payloads, so it
/// cannot hold [`JsonValue`] directly.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(JsonNumber),
    String(String),
    Array(Vec<JsonValue>),
    Object(JsonObject),
}

/// Insertion-ordered string-keyed map backing [`JsonValue::Object`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsonObject {
    entries: Vec<(String, JsonValue)>,
}

impl JsonObject {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Inserts a key/value pair. Inserting a key that already exists replaces
    /// its value in place and never moves the entry's position.
    pub fn insert(&mut self, key: String, value: JsonValue) -> Option<JsonValue> {
        if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|(existing, _)| *existing == key)
        {
            return Some(std::mem::replace(&mut slot.1, value));
        }
        self.entries.push((key, value));
        None
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.entries
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &JsonValue)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(key, _)| key.as_str())
    }
}

impl FromIterator<(String, JsonValue)> for JsonObject {
    fn from_iter<I: IntoIterator<Item = (String, JsonValue)>>(iter: I) -> Self {
        let mut object = JsonObject::new();
        for (key, value) in iter {
            object.insert(key, value);
        }
        object
    }
}

impl JsonValue {
    pub fn kind(&self) -> SpectraHostValue {
        match self {
            JsonValue::Null => JSON_KIND_NULL,
            JsonValue::Bool(_) => JSON_KIND_BOOL,
            JsonValue::Number(_) => JSON_KIND_NUMBER,
            JsonValue::String(_) => JSON_KIND_STRING,
            JsonValue::Array(_) => JSON_KIND_ARRAY,
            JsonValue::Object(_) => JSON_KIND_OBJECT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JsonNumber {
    repr: String,
}

impl JsonNumber {
    pub fn from_i64(value: i64) -> Self {
        Self {
            repr: value.to_string(),
        }
    }

    pub fn from_u64(value: u64) -> Self {
        Self {
            repr: value.to_string(),
        }
    }

    pub fn from_f64(value: f64) -> Result<Self, JsonEncodeError> {
        let number = Number::from_f64(value).ok_or_else(|| {
            JsonEncodeError::new(
                JsonEncodeErrorKind::NonFiniteNumber,
                "JSON numbers cannot encode NaN or infinity",
            )
        })?;
        Ok(Self {
            repr: number.to_string(),
        })
    }

    pub fn parse(text: impl Into<String>) -> Result<Self, JsonEncodeError> {
        let repr = text.into();
        Number::from_str(&repr).map_err(|error| {
            JsonEncodeError::with_cause(
                JsonEncodeErrorKind::InvalidNumber,
                "invalid JSON number representation",
                error.to_string(),
            )
        })?;
        Ok(Self { repr })
    }

    pub fn as_str(&self) -> &str {
        &self.repr
    }

    pub fn as_i64(&self) -> Option<i64> {
        Number::from_str(&self.repr).ok()?.as_i64()
    }

    pub fn as_u64(&self) -> Option<u64> {
        Number::from_str(&self.repr).ok()?.as_u64()
    }

    pub fn as_f64(&self) -> Option<f64> {
        Number::from_str(&self.repr).ok()?.as_f64()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonParseErrorKind {
    InvalidSyntax,
    UnexpectedEof,
    InvalidData,
    Io,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonParseError {
    pub kind: JsonParseErrorKind,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl JsonParseError {
    fn from_serde(text: &str, error: serde_json::Error) -> Self {
        let line = error.line().max(1);
        let column = error.column().max(1);
        let offset = byte_offset_for_line_column(text, line, column);
        let kind = match error.classify() {
            serde_json::error::Category::Io => JsonParseErrorKind::Io,
            serde_json::error::Category::Syntax => JsonParseErrorKind::InvalidSyntax,
            serde_json::error::Category::Data => JsonParseErrorKind::InvalidData,
            serde_json::error::Category::Eof => JsonParseErrorKind::UnexpectedEof,
        };
        Self {
            kind,
            offset,
            line,
            column,
            message: error.to_string(),
        }
    }
}

impl fmt::Display for JsonParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at byte {} (line {}, column {})",
            self.message, self.offset, self.line, self.column
        )
    }
}

impl std::error::Error for JsonParseError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonEncodeErrorKind {
    InvalidNumber,
    NonFiniteNumber,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonEncodeError {
    pub kind: JsonEncodeErrorKind,
    pub message: String,
    pub cause: Option<String>,
}

impl JsonEncodeError {
    fn new(kind: JsonEncodeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            cause: None,
        }
    }

    fn with_cause(
        kind: JsonEncodeErrorKind,
        message: impl Into<String>,
        cause: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            cause: Some(cause.into()),
        }
    }
}

impl fmt::Display for JsonEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cause {
            Some(cause) => write!(f, "{}: {}", self.message, cause),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for JsonEncodeError {}

pub fn parse_json(text: &str) -> Result<JsonValue, JsonParseError> {
    let value = serde_json::from_str::<Value>(text)
        .map_err(|error| JsonParseError::from_serde(text, error))?;
    Ok(from_serde_value(value))
}

pub fn encode_json(value: &JsonValue) -> Result<String, JsonEncodeError> {
    serde_json::to_string(&to_serde_value(value)?).map_err(|error| {
        JsonEncodeError::with_cause(
            JsonEncodeErrorKind::InvalidNumber,
            "failed to encode JSON value",
            error.to_string(),
        )
    })
}

pub fn encode_json_pretty(value: &JsonValue) -> Result<String, JsonEncodeError> {
    serde_json::to_string_pretty(&to_serde_value(value)?).map_err(|error| {
        JsonEncodeError::with_cause(
            JsonEncodeErrorKind::InvalidNumber,
            "failed to encode pretty JSON value",
            error.to_string(),
        )
    })
}

pub fn json_kind_of(text: &str) -> SpectraHostValue {
    parse_json(text)
        .map(|value| value.kind())
        .unwrap_or(JSON_KIND_INVALID)
}

fn from_serde_value(value: Value) -> JsonValue {
    match value {
        Value::Null => JsonValue::Null,
        Value::Bool(value) => JsonValue::Bool(value),
        Value::Number(value) => JsonValue::Number(JsonNumber {
            repr: value.to_string(),
        }),
        Value::String(value) => JsonValue::String(value),
        Value::Array(values) => {
            JsonValue::Array(values.into_iter().map(from_serde_value).collect())
        }
        Value::Object(values) => JsonValue::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, from_serde_value(value)))
                .collect(),
        ),
    }
}

fn to_serde_value(value: &JsonValue) -> Result<Value, JsonEncodeError> {
    match value {
        JsonValue::Null => Ok(Value::Null),
        JsonValue::Bool(value) => Ok(Value::Bool(*value)),
        JsonValue::Number(value) => {
            Number::from_str(value.as_str())
                .map(Value::Number)
                .map_err(|error| {
                    JsonEncodeError::with_cause(
                        JsonEncodeErrorKind::InvalidNumber,
                        "invalid JSON number representation",
                        error.to_string(),
                    )
                })
        }
        JsonValue::String(value) => Ok(Value::String(value.clone())),
        JsonValue::Array(values) => values
            .iter()
            .map(to_serde_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        JsonValue::Object(values) => values
            .iter()
            .map(|(key, value)| Ok((key.to_string(), to_serde_value(value)?)))
            .collect::<Result<Map<String, _>, JsonEncodeError>>()
            .map(Value::Object),
    }
}

fn byte_offset_for_line_column(text: &str, line: usize, column: usize) -> usize {
    let target_line = line.saturating_sub(1);
    let target_column = column.saturating_sub(1);
    let mut current_line = 0usize;
    let mut current_column = 0usize;
    for (offset, ch) in text.char_indices() {
        if current_line == target_line && current_column == target_column {
            return offset;
        }
        if ch == '\n' {
            current_line += 1;
            current_column = 0;
        } else {
            current_column += 1;
        }
    }
    text.len()
}

pub extern "C" fn json_validate(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let valid = read_spectra_string(args[0])
        .map(|text| parse_json(&text).is_ok())
        .unwrap_or(false);
    write_result(ctx, valid as SpectraHostValue)
}

pub extern "C" fn json_kind(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let kind = read_spectra_string(args[0])
        .map(|text| json_kind_of(&text))
        .unwrap_or(JSON_KIND_INVALID);
    write_result(ctx, kind)
}

struct JsonStore {
    values: ApiHandleTable<Value>,
}

fn json_store() -> &'static Mutex<JsonStore> {
    static STORE: LazyLock<Mutex<JsonStore>> = LazyLock::new(|| {
        Mutex::new(JsonStore {
            values: ApiHandleTable::new(spectra_runtime::handles::HandleKind::ApiJsonValue),
        })
    });
    &STORE
}

fn insert_json_value(value: Value) -> SpectraHostValue {
    match json_store().lock() {
        Ok(mut store) => store.values.insert(value),
        Err(_) => 0,
    }
}

fn stored_json_value(raw: SpectraHostValue) -> Option<Value> {
    let store = &mut *json_store().lock().ok()?;
    store.values.get(&raw).cloned()
}

/// Looks up one object member by key under a single store lock, cloning only
/// the child. Unlike `value_get` (which clones the parent, then the child,
/// then inserts a fresh handle), this leaves the parent untouched and
/// creates no handle: the caller owns the returned value outright.
fn stored_json_child(obj: SpectraHostValue, key: &str) -> Option<Value> {
    let store = &mut *json_store().lock().ok()?;
    store.values.get(&obj)?.get(key).cloned()
}

fn json_kind_of_serde(value: &Value) -> SpectraHostValue {
    match value {
        Value::Null => JSON_KIND_NULL,
        Value::Bool(_) => JSON_KIND_BOOL,
        Value::Number(_) => JSON_KIND_NUMBER,
        Value::String(_) => JSON_KIND_STRING,
        Value::Array(_) => JSON_KIND_ARRAY,
        Value::Object(_) => JSON_KIND_OBJECT,
    }
}

pub extern "C" fn json_parse(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let parsed = read_spectra_string(args[0])
        .and_then(|text| parse_json(&text).ok())
        .and_then(|value| to_serde_value(&value).ok());
    let Some(value) = parsed else {
        return write_result(ctx, 0);
    };
    write_result(ctx, insert_json_value(value))
}

pub extern "C" fn json_value_kind(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let kind = stored_json_value(args[0])
        .map(|value| json_kind_of_serde(&value))
        .unwrap_or(JSON_KIND_INVALID);
    write_result(ctx, kind)
}

pub extern "C" fn json_value_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let len = stored_json_value(args[0])
        .map(|value| match value {
            Value::Array(values) => values.len() as SpectraHostValue,
            Value::Object(values) => values.len() as SpectraHostValue,
            _ => 0,
        })
        .unwrap_or(0);
    write_result(ctx, len)
}

pub extern "C" fn json_value_get(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let child = read_spectra_string(args[1])
        .and_then(|key| stored_json_value(args[0]).and_then(|value| value.get(key).cloned()));
    write_result(ctx, child.map(insert_json_value).unwrap_or(0))
}

pub extern "C" fn json_value_at(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let child = stored_json_value(args[0]).and_then(|value| {
        let index = usize::try_from(args[1]).ok()?;
        value.get(index).cloned()
    });
    write_result(ctx, child.map(insert_json_value).unwrap_or(0))
}

pub extern "C" fn json_value_text(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let text = stored_json_value(args[0])
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&text))
}

pub extern "C" fn json_value_number_bits(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let bits = stored_json_value(args[0])
        .and_then(|value| value.as_f64())
        .map(|number| number.to_bits() as i64)
        .unwrap_or(0);
    write_result(ctx, bits)
}

pub extern "C" fn json_value_bool(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let flag = stored_json_value(args[0])
        .map(|value| match value {
            Value::Bool(true) => 1,
            Value::Bool(false) => 0,
            _ => -1,
        })
        .unwrap_or(-1);
    write_result(ctx, flag)
}

pub extern "C" fn json_value_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if let Ok(mut store) = json_store().lock() {
        store.values.remove(&args[0]);
    }
    HOST_STATUS_SUCCESS
}

pub extern "C" fn json_stringify(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let encoded = stored_json_value(args[0])
        .and_then(|value| serde_json::to_string(&value).ok())
        .unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&encoded))
}
/// Derive support: quote a raw Spectra string as a JSON string literal.
///
/// Used by `#[derive(Serialize)]` lowering for string fields. Escaping follows
/// `serde_json` so embedded quotes, backslashes, and control characters can
/// never break the encoded object.
pub extern "C" fn json_quote_string(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(text) = read_spectra_string(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let quoted = serde_json::to_string(&text).unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&quoted))
}

/// Derive support: quote a single Unicode scalar as a JSON string literal.
///
/// Used by `#[derive(Serialize)]` lowering for `char` fields. There is no
/// `char` to `string` host elsewhere, and routing codepoints through
/// `int_to_string` would emit `65` instead of `"A"`.
pub extern "C" fn json_quote_char(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let codepoint = args[0] as u32;
    let Some(ch) = char::from_u32(codepoint) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut text = String::with_capacity(ch.len_utf8());
    text.push(ch);
    let quoted = serde_json::to_string(&text).unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&quoted))
}

/// Derive support: format f64 bits as a canonical JSON number.
///
/// Used by `#[derive(Serialize)]` lowering for `float` fields. The generic
/// `float_to_string` conversion emits Rust `Display` (`7` for `7.0`), which
/// would change the JSON type on round-trip; non-finite values are rejected
/// with an error status instead of emitting invalid JSON.
pub extern "C" fn json_encode_number(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let value = f64::from_bits(args[0] as u64);
    let Some(number) = Number::from_f64(value) else {
        eprintln!("spectra.api.json encode error: non-finite float cannot be encoded as JSON");
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&number.to_string()))
}

/// Report a typed decode failure to stderr and fail the host call.
///
/// The backend turns the error status into `runtime error: host call ...`,
/// exit 101; this line names the exact JSON path first so the diagnostic
/// points at the offending field.
fn decode_failure(path: &str, reason: &str) -> i32 {
    eprintln!("spectra.api.json decode error at '{path}': {reason}");
    HOST_STATUS_INVALID_ARGUMENT
}

/// Derive support: extract and validate one field for `#[derive(Deserialize)]`.
///
/// Arguments: `(child_handle, path, type_name, optional, default_value)`.
/// `child_handle` is the total-lookup result (`value_get`/`value_at`/`parse`):
/// 0 (or an unknown handle, or JSON null) means absent. `type_name` is one of
/// `int`, `float`, `bool`, `string`, `char`; anything else selects object
/// mode for nested derived structs and returns the child handle unchanged.
/// Absent `optional` fields yield `default_value`; any other violation fails
/// loudly instead of synthesizing a silent zero.
pub extern "C" fn json_decode_field(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let path = read_spectra_string(args[1]).unwrap_or_else(|| "$".to_string());
    let type_name = read_spectra_string(args[2]).unwrap_or_default();
    let optional = args[3] != 0;
    let default = args[4];
    let field = if args[0] == 0 {
        None
    } else {
        stored_json_value(args[0]).filter(|value| !value.is_null())
    };
    // Object mode echoes the child handle: it already names the nested
    // object, so no new store entry is needed.
    write_decoded_field(ctx, field, &path, &type_name, optional, default, Some(args[0]))
}

/// Derive support: `value_get` + `decode_field` collapsed into one host call.
///
/// Arguments: `(obj_handle, key, path, type_name, optional, default_value)`.
/// Looks the member up by key without cloning the parent and without
/// creating a child handle. Scalar fields are extracted directly; nested
/// objects (any other `type_name`) are moved into a fresh handle which the
/// caller must release with `value_free`.
pub extern "C" fn json_decode_field_by_key(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 6) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let key = read_spectra_string(args[1]).unwrap_or_default();
    let path = read_spectra_string(args[2]).unwrap_or_else(|| "$".to_string());
    let type_name = read_spectra_string(args[3]).unwrap_or_default();
    let optional = args[4] != 0;
    let default = args[5];
    let field = stored_json_child(args[0], &key).filter(|value| !value.is_null());
    // Object mode has no incoming child handle, so the nested object is
    // moved into a fresh store entry (`None` selects the insert path).
    write_decoded_field(ctx, field, &path, &type_name, optional, default, None)
}

/// Shared extraction for `json_decode_field` and
/// `json_decode_field_by_key`. `object_handle` selects object-mode behavior
/// for nested derived structs: `Some(handle)` echoes an existing child
/// handle, `None` moves the owned `field` into a fresh store entry.
fn write_decoded_field(
    ctx: *mut SpectraHostCallContext,
    field: Option<Value>,
    path: &str,
    type_name: &str,
    optional: bool,
    default: SpectraHostValue,
    object_handle: Option<SpectraHostValue>,
) -> i32 {
    let missing = |reason: String| decode_failure(path, &reason);
    let Some(field) = field else {
        if optional {
            return write_result(ctx, default);
        }
        if path == "$" {
            return missing("invalid JSON document: expected object at root".to_string());
        }
        return missing("missing required field".to_string());
    };
    match type_name {
        "bool" => match field.as_bool() {
            Some(flag) => write_result(ctx, i64::from(flag)),
            None => missing(format!(
                "expected boolean, found {}",
                json_kind_name(&field)
            )),
        },
        "int" => match field
            .as_i64()
            .or_else(|| field.as_u64().and_then(|value| i64::try_from(value).ok()))
        {
            Some(number) => write_result(ctx, number),
            None if field.is_number() => {
                missing("expected integer, found non-integral number".to_string())
            }
            None => missing(format!(
                "expected integer, found {}",
                json_kind_name(&field)
            )),
        },
        "float" => match field.as_f64() {
            Some(number) => write_result(ctx, number.to_bits() as i64),
            None => missing("expected number".to_string()),
        },
        "string" => match field.as_str() {
            Some(text) => write_result(ctx, alloc_spectra_string(text)),
            None => missing("expected string".to_string()),
        },
        "char" => match field.as_str() {
            Some(text) if text.chars().count() == 1 => {
                write_result(ctx, text.chars().next().unwrap_or_default() as i64)
            }
            _ => missing("expected single-character string".to_string()),
        },
        _ => {
            if !field.is_object() {
                return missing("expected object".to_string());
            }
            match object_handle {
                Some(handle) => write_result(ctx, handle),
                // No incoming handle: move the nested object into a fresh
                // store entry the caller releases with `value_free`.
                None => write_result(ctx, insert_json_value(field)),
            }
        }
    }
}

/// Compact schema DSL consumed by [`json_typed_error_field`], generated by
/// midend derive lowering (never hand-written):
/// `Type{f1:int!;f2:string?;nested:{x:float!};tags:[int]!}`.
/// `!` marks required fields, `?` optional ones; paths join with `.` and
/// array indices render as `[i]`. Root problems report as `$`.
#[derive(Clone, Debug, PartialEq)]
enum DeriveSchemaType {
    Int,
    Float,
    Bool,
    String,
    Char,
    Object(Vec<DeriveSchemaField>),
    Array(Box<DeriveSchemaType>),
}

#[derive(Clone, Debug, PartialEq)]
struct DeriveSchemaField {
    json_name: String,
    optional: bool,
    ty: DeriveSchemaType,
}

struct SchemaParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> SchemaParser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn eat(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_name(&mut self) -> Option<String> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
        ) {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        Some(String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned())
    }

    fn parse_type(&mut self) -> Option<DeriveSchemaType> {
        match self.peek() {
            Some(b'{') => {
                self.pos += 1;
                let mut fields = Vec::new();
                if self.eat(b'}') {
                    return Some(DeriveSchemaType::Object(fields));
                }
                loop {
                    let json_name = self.parse_name()?;
                    self.eat(b':');
                    let ty = self.parse_type()?;
                    let optional = if self.eat(b'?') {
                        true
                    } else if self.eat(b'!') {
                        false
                    } else {
                        return None;
                    };
                    fields.push(DeriveSchemaField {
                        json_name,
                        optional,
                        ty,
                    });
                    if self.eat(b';') {
                        if self.eat(b'}') {
                            return Some(DeriveSchemaType::Object(fields));
                        }
                        continue;
                    }
                    if self.eat(b'}') {
                        return Some(DeriveSchemaType::Object(fields));
                    }
                    return None;
                }
            }
            Some(b'[') => {
                self.pos += 1;
                let element = self.parse_type()?;
                if !self.eat(b']') {
                    return None;
                }
                Some(DeriveSchemaType::Array(Box::new(element)))
            }
            _ => {
                let name = self.parse_name()?;
                match name.as_str() {
                    "int" => Some(DeriveSchemaType::Int),
                    "float" => Some(DeriveSchemaType::Float),
                    "bool" => Some(DeriveSchemaType::Bool),
                    "string" => Some(DeriveSchemaType::String),
                    "char" => Some(DeriveSchemaType::Char),
                    _ => None,
                }
            }
        }
    }

    fn parse_schema(&mut self) -> Option<Vec<DeriveSchemaField>> {
        self.parse_name()?;
        match self.parse_type()? {
            DeriveSchemaType::Object(fields) => {
                if self.pos == self.bytes.len() {
                    Some(fields)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

fn json_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn check_schema_type(value: &Value, ty: &DeriveSchemaType, path: &str) -> Option<String> {
    match ty {
        DeriveSchemaType::Int => {
            let integral = value.as_i64().is_some()
                || value
                    .as_u64()
                    .and_then(|number| i64::try_from(number).ok())
                    .is_some();
            if integral {
                None
            } else {
                Some(path.to_string())
            }
        }
        DeriveSchemaType::Float => {
            if value.as_f64().is_some() {
                None
            } else {
                Some(path.to_string())
            }
        }
        DeriveSchemaType::Bool => {
            if value.is_boolean() {
                None
            } else {
                Some(path.to_string())
            }
        }
        DeriveSchemaType::String => {
            if value.is_string() {
                None
            } else {
                Some(path.to_string())
            }
        }
        DeriveSchemaType::Char => {
            let single = value
                .as_str()
                .map(|text| text.chars().count() == 1)
                .unwrap_or(false);
            if single {
                None
            } else {
                Some(path.to_string())
            }
        }
        DeriveSchemaType::Object(fields) => {
            let Some(object) = value.as_object() else {
                return Some(path.to_string());
            };
            for field in fields {
                let field_path = format!("{path}.{}", field.json_name);
                match object.get(&field.json_name) {
                    None => {
                        if !field.optional {
                            return Some(field_path);
                        }
                    }
                    Some(item) if item.is_null() => {
                        if !field.optional {
                            return Some(field_path);
                        }
                    }
                    Some(item) => {
                        if let Some(bad) = check_schema_type(item, &field.ty, &field_path) {
                            return Some(bad);
                        }
                    }
                }
            }
            None
        }
        DeriveSchemaType::Array(element) => {
            let Some(items) = value.as_array() else {
                return Some(path.to_string());
            };
            for (index, item) in items.iter().enumerate() {
                if item.is_null() {
                    return Some(format!("{path}[{index}]"));
                }
                if let Some(bad) = check_schema_type(item, element, &format!("{path}[{index}]")) {
                    return Some(bad);
                }
            }
            None
        }
    }
}

/// Derive support: report the first JSON path violating a derived schema.
///
/// Arguments: `(schema, input)`. Returns `""` when `input` satisfies the
/// schema, otherwise the offending path (`user_id`, `address.city`,
/// `tags[2]`; `$` for root problems such as invalid syntax or a non-object
/// root). A malformed schema (a compiler bug, never user input) also yields
/// `$`: this host never reports valid input it could not check.
pub extern "C" fn json_typed_error_field(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(schema_text), Some(input)) =
        (read_spectra_string(args[0]), read_spectra_string(args[1]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut parser = SchemaParser::new(&schema_text);
    let bad = match parser.parse_schema() {
        None => Some("$".to_string()),
        Some(fields) => match serde_json::from_str::<Value>(&input) {
            Err(_) => Some("$".to_string()),
            Ok(value) if !value.is_object() => Some("$".to_string()),
            Ok(value) => check_schema_type(&value, &DeriveSchemaType::Object(fields), "$")
                .map(|path| path.strip_prefix("$.").unwrap_or(&path).to_string()),
        },
    };
    write_result(ctx, alloc_spectra_string(bad.as_deref().unwrap_or("")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(entries: Vec<(&str, JsonValue)>) -> JsonValue {
        JsonValue::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    #[test]
    fn round_trip_primitives_arrays_maps_nested_and_null() {
        let value = object(vec![
            ("null", JsonValue::Null),
            ("bool", JsonValue::Bool(true)),
            ("int", JsonValue::Number(JsonNumber::from_i64(-42))),
            (
                "float",
                JsonValue::Number(JsonNumber::from_f64(12.5).expect("finite number")),
            ),
            (
                "string",
                JsonValue::String("quote: \" slash: \\ newline:\n tab:\t".to_string()),
            ),
            (
                "array",
                JsonValue::Array(vec![
                    JsonValue::Null,
                    JsonValue::Bool(false),
                    JsonValue::String("nested".to_string()),
                ]),
            ),
            (
                "map",
                object(vec![(
                    "child",
                    JsonValue::Array(vec![JsonValue::Number(JsonNumber::from_u64(7))]),
                )]),
            ),
        ]);

        let encoded = encode_json(&value).expect("encode JSON");
        let decoded = parse_json(&encoded).expect("decode JSON");
        assert_eq!(decoded, value);
        assert_eq!(json_kind_of(&encoded), JSON_KIND_OBJECT);
    }

    #[test]
    fn parser_handles_common_escape_sequences_and_unicode() {
        let decoded = parse_json(r#"{"text":"line\nquote\"slash\\tab\tunicode \u263A"}"#)
            .expect("escaped JSON parses");
        let JsonValue::Object(map) = decoded else {
            panic!("expected object");
        };
        assert_eq!(
            map.get("text"),
            Some(&JsonValue::String(
                "line\nquote\"slash\\tab\tunicode ☺".to_string()
            ))
        );
    }

    #[test]
    fn invalid_json_reports_typed_error_with_byte_offset() {
        let err = parse_json("{\n  \"ok\": true,\n  bad\n}")
            .expect_err("invalid JSON must fail with typed offset");
        assert_eq!(err.kind, JsonParseErrorKind::InvalidSyntax);
        assert_eq!(err.line, 3);
        assert!(err.offset >= "{\n  \"ok\": true,\n  ".len());
        assert!(err.message.contains("key") || err.message.contains("expected"));
    }

    #[test]
    fn encoder_rejects_invalid_numbers_and_non_finite_float_values() {
        let nan = JsonNumber::from_f64(f64::NAN).expect_err("NaN is not JSON");
        assert_eq!(nan.kind, JsonEncodeErrorKind::NonFiniteNumber);

        let invalid = JsonValue::Number(JsonNumber {
            repr: "01".to_string(),
        });
        let err = encode_json(&invalid).expect_err("invalid number repr");
        assert_eq!(err.kind, JsonEncodeErrorKind::InvalidNumber);
    }
    #[test]
    fn derive_schema_reports_first_bad_path() {
        let mut parser = SchemaParser::new("Profile{user_id:int!;name:string!;nickname:string?}");
        let fields = parser.parse_schema().expect("schema parses");
        let check = |input: &str| {
            let value: Value = serde_json::from_str(input).expect("test input parses");
            check_schema_type(&value, &DeriveSchemaType::Object(fields.clone()), "$")
                .map(|path| path.strip_prefix("$.").unwrap_or(&path).to_string())
        };
        assert_eq!(check(r#"{"user_id":7,"name":"Ada"}"#), None);
        assert_eq!(check(r#"{"user_id":7,"name":"Ada","nickname":null}"#), None);
        assert_eq!(check(r#"{"name":"Ada"}"#).as_deref(), Some("user_id"));
        assert_eq!(
            check(r#"{"user_id":"7","name":"Ada"}"#).as_deref(),
            Some("user_id")
        );
        assert_eq!(
            check(r#"{"user_id":7.5,"name":"Ada"}"#).as_deref(),
            Some("user_id")
        );
    }
    #[test]
    fn derive_schema_accepts_trailing_field_separator() {
        let mut parser = SchemaParser::new("Profile{id:int!;tags:{a:string!;}!;}");
        let fields = parser.parse_schema().expect("trailing separators parse");
        assert_eq!(fields.len(), 2);
        let value: Value = serde_json::from_str(r#"{"id":1,"tags":{"a":"x"}}"#).unwrap();
        assert_eq!(
            check_schema_type(&value, &DeriveSchemaType::Object(fields), "$"),
            None
        );
    }

    #[test]
    fn derive_schema_checks_nested_objects_and_arrays() {
        let mut parser = SchemaParser::new("Order{id:int!;address:{city:string!}!;tags:[string]!}");
        let fields = parser.parse_schema().expect("schema parses");
        let check = |input: &str| {
            let value: Value = serde_json::from_str(input).expect("test input parses");
            check_schema_type(&value, &DeriveSchemaType::Object(fields.clone()), "$")
                .map(|path| path.strip_prefix("$.").unwrap_or(&path).to_string())
        };
        assert_eq!(
            check(r#"{"id":1,"address":{"city":"Lima"},"tags":["a"]}"#),
            None
        );
        assert_eq!(
            check(r#"{"id":1,"address":{},"tags":[]}"#).as_deref(),
            Some("address.city")
        );
        assert_eq!(
            check(r#"{"id":1,"address":{"city":"Lima"},"tags":["a",4]}"#).as_deref(),
            Some("tags[1]")
        );
        assert_eq!(
            check(r#"{"id":1,"address":{"city":"Lima"},"tags":{}}"#).as_deref(),
            Some("tags")
        );
    }

    #[test]
    fn derive_schema_rejects_malformed_schemas_and_non_objects() {
        assert!(SchemaParser::new("Profile{user_id:int}")
            .parse_schema()
            .is_none());
        assert!(SchemaParser::new("Profile{user_id:unknown!}")
            .parse_schema()
            .is_none());
        assert!(SchemaParser::new("").parse_schema().is_none());
        let mut parser = SchemaParser::new("Profile{id:int!}");
        let fields = parser.parse_schema().expect("schema parses");
        let root = DeriveSchemaType::Object(fields);
        assert_eq!(
            check_schema_type(&Value::Null, &root, "$").as_deref(),
            Some("$")
        );
        assert_eq!(
            check_schema_type(&Value::Array(vec![]), &root, "$").as_deref(),
            Some("$")
        );
    }

    #[test]
    fn encoder_output_is_rfc8259_json_for_supported_values() {
        let value = JsonValue::Array(vec![
            JsonValue::String("\u{0008}\u{000c}\r\n".to_string()),
            object(vec![(
                "x",
                JsonValue::Number(JsonNumber::parse("1e-9").unwrap()),
            )]),
        ]);
        let encoded = encode_json(&value).expect("encode supported JSON");
        serde_json::from_str::<Value>(&encoded).expect("serde accepts encoded RFC 8259 JSON");
        let reparsed = parse_json(&encoded).expect("self parser accepts encoded JSON");
        assert_eq!(reparsed, value);
    }

    #[test]
    fn parse_preserves_object_key_insertion_order() {
        let decoded = parse_json(r#"{"b":1,"a":2}"#).expect("parse");
        let encoded = encode_json(&decoded).expect("encode");
        assert_eq!(encoded, r#"{"b":1,"a":2}"#);

        let nested =
            parse_json(r#"{"z":{"y":1,"x":2},"w":[{"d":1,"c":2}]}"#).expect("parse nested");
        let JsonValue::Object(root) = &nested else {
            panic!("expected object");
        };
        let keys: Vec<&str> = root.keys().collect();
        assert_eq!(keys, ["z", "w"], "top-level key order must follow input");
        let Some(JsonValue::Object(inner)) = root.get("z") else {
            panic!("expected nested object");
        };
        let inner_keys: Vec<&str> = inner.keys().collect();
        assert_eq!(inner_keys, ["y", "x"], "nested key order must follow input");
    }

    #[test]
    fn updating_existing_object_key_preserves_position() {
        let mut map = JsonObject::new();
        map.insert("b".to_string(), JsonValue::Number(JsonNumber::from_i64(1)));
        map.insert("a".to_string(), JsonValue::Number(JsonNumber::from_i64(2)));
        map.insert("b".to_string(), JsonValue::Number(JsonNumber::from_i64(9)));

        let encoded = encode_json(&JsonValue::Object(map)).expect("encode");
        assert_eq!(
            encoded, r#"{"b":9,"a":2}"#,
            "update must replace the value without moving the key"
        );
    }
    #[test]
    fn host_kind_uses_full_parser_not_balanced_braces() {
        assert_eq!(json_kind_of(r#"{"unterminated":["#), JSON_KIND_INVALID);
        assert_eq!(json_kind_of(r#"[{"ok":true}, null]"#), JSON_KIND_ARRAY);
        assert_eq!(json_kind_of(r#""hello""#), JSON_KIND_STRING);
        assert_eq!(json_kind_of("-3.5e+7"), JSON_KIND_NUMBER);
    }

    fn call_json_host(
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

    fn call_json_host_no_result(
        function: extern "C" fn(*mut SpectraHostCallContext) -> i32,
        args: &[SpectraHostValue],
    ) -> i32 {
        let mut ctx = SpectraHostCallContext {
            args: args.as_ptr(),
            arg_len: args.len(),
            results: std::ptr::null_mut(),
            result_len: 0,
            invoke_fn: None,
        };
        function(&mut ctx)
    }

    const SAMPLE: &str = r#"{"name":"Ada","count":2,"ratio":2.5,"flag":true,"off":false,"none":null,"tags":[10,20,30]}"#;

    #[test]
    fn host_parse_accepts_valid_and_rejects_invalid_documents() {
        let text = alloc_spectra_string(SAMPLE);
        let (status, handle) = call_json_host(json_parse, &[text]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert!(handle != 0, "valid document must yield a nonzero handle");

        let bad = alloc_spectra_string("{\"unterminated\":");
        let (status, handle) = call_json_host(json_parse, &[bad]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(handle, 0, "invalid document must yield the zero handle");

        let mut ctx = SpectraHostCallContext {
            args: std::ptr::null(),
            arg_len: 0,
            results: std::ptr::null_mut(),
            result_len: 0,
            invoke_fn: None,
        };
        assert_eq!(json_parse(&mut ctx), HOST_STATUS_INVALID_ARGUMENT);
    }

    #[test]
    fn host_navigation_walks_objects_arrays_and_reports_lengths() {
        let (_, root) = call_json_host(json_parse, &[alloc_spectra_string(SAMPLE)]);

        let (status, kind) = call_json_host(json_value_kind, &[root]);
        assert_eq!((status, kind), (HOST_STATUS_SUCCESS, JSON_KIND_OBJECT));
        let (_, len) = call_json_host(json_value_len, &[root]);
        assert_eq!(len, 7);

        let (_, tags) = call_json_host(json_value_get, &[root, alloc_spectra_string("tags")]);
        assert!(tags != 0);
        let (_, tags_kind) = call_json_host(json_value_kind, &[tags]);
        assert_eq!(tags_kind, JSON_KIND_ARRAY);
        let (_, tags_len) = call_json_host(json_value_len, &[tags]);
        assert_eq!(tags_len, 3);

        let (_, first) = call_json_host(json_value_at, &[tags, 0]);
        let (_, first_bits) = call_json_host(json_value_number_bits, &[first]);
        assert_eq!(f64::from_bits(first_bits as u64), 10.0);
        let (_, third) = call_json_host(json_value_at, &[tags, 2]);
        assert!(third != 0);
        assert_eq!(
            call_json_host(json_value_at, &[tags, 3]).1,
            0,
            "out-of-range index must yield the zero handle"
        );
        assert_eq!(
            call_json_host(json_value_at, &[tags, -1]).1,
            0,
            "negative index must yield the zero handle"
        );

        assert_eq!(
            call_json_host(json_value_get, &[root, alloc_spectra_string("missing")]).1,
            0
        );

        let (_, name) = call_json_host(json_value_get, &[root, alloc_spectra_string("name")]);
        let (_, name_ptr) = call_json_host(json_value_text, &[name]);
        assert_eq!(read_spectra_string(name_ptr).as_deref(), Some("Ada"));
    }

    #[test]
    fn host_accessors_report_sentinels_for_wrong_value_types() {
        let (_, root) = call_json_host(json_parse, &[alloc_spectra_string(SAMPLE)]);

        let flag = call_json_host(json_value_get, &[root, alloc_spectra_string("flag")]).1;
        assert_eq!(call_json_host(json_value_bool, &[flag]).1, 1);
        let off = call_json_host(json_value_get, &[root, alloc_spectra_string("off")]).1;
        assert_eq!(call_json_host(json_value_bool, &[off]).1, 0);
        let none = call_json_host(json_value_get, &[root, alloc_spectra_string("none")]).1;
        assert_eq!(call_json_host(json_value_bool, &[none]).1, -1);
        assert_eq!(
            call_json_host(json_value_bool, &[root]).1,
            -1,
            "non-bool values must report -1"
        );

        let ratio = call_json_host(json_value_get, &[root, alloc_spectra_string("ratio")]).1;
        assert_eq!(
            f64::from_bits(call_json_host(json_value_number_bits, &[ratio]).1 as u64),
            2.5
        );
        assert_eq!(
            call_json_host(json_value_number_bits, &[root]).1,
            0,
            "non-number values must report zero bits"
        );

        assert_eq!(
            read_spectra_string(call_json_host(json_value_text, &[root]).1).as_deref(),
            Some("")
        );

        let (_, count) = call_json_host(json_value_get, &[root, alloc_spectra_string("count")]);
        assert_eq!(call_json_host(json_value_len, &[count]).1, 0);
    }

    #[test]
    fn host_stringify_round_trips_parsed_documents() {
        let (_, root) = call_json_host(json_parse, &[alloc_spectra_string(SAMPLE)]);
        let (_, encoded_ptr) = call_json_host(json_stringify, &[root]);
        let encoded = read_spectra_string(encoded_ptr).expect("stringify returns a string");

        let reparsed: Value = serde_json::from_str(&encoded).expect("compact output is JSON");
        assert_eq!(reparsed["name"], "Ada");
        assert_eq!(reparsed["tags"].as_array().map(Vec::len), Some(3));

        let (_, second) = call_json_host(json_parse, &[alloc_spectra_string(&encoded)]);
        let (_, second_encoded_ptr) = call_json_host(json_stringify, &[second]);
        assert_eq!(
            read_spectra_string(second_encoded_ptr).as_deref(),
            Some(encoded.as_str())
        );

        assert_eq!(
            read_spectra_string(call_json_host(json_stringify, &[0]).1).as_deref(),
            Some(""),
            "invalid handles stringify to the empty string"
        );
    }

    #[test]
    fn host_free_invalidates_handles() {
        let (_, root) = call_json_host(json_parse, &[alloc_spectra_string(SAMPLE)]);
        let status = call_json_host_no_result(json_value_free, &[root]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(
            call_json_host(json_value_kind, &[root]).1,
            JSON_KIND_INVALID,
            "freed handles must classify as invalid"
        );
        // Freeing an unknown handle is a no-op that still succeeds.
        assert_eq!(
            call_json_host_no_result(json_value_free, &[0]),
            HOST_STATUS_SUCCESS
        );
    }

    #[test]
    fn host_parse_get_free_cycle_releases_handles() {
        // Mirrors one derived `from_json` field access: parse a document,
        // look up a member, then free the child and the root. Freed handles
        // must classify as invalid even across repeated cycles, so a loop
        // of roundtrips cannot accumulate live entries in the JSON store.
        // Only per-handle state is asserted: the store is process-global and
        // shared with other tests running on other threads.
        for _ in 0..32 {
            let (_, root) = call_json_host(json_parse, &[alloc_spectra_string(SAMPLE)]);
            assert_ne!(root, 0, "valid document must yield a nonzero handle");
            let (_, child) =
                call_json_host(json_value_get, &[root, alloc_spectra_string("name")]);
            assert_ne!(child, 0, "present member must yield a nonzero handle");
            assert_eq!(
                call_json_host_no_result(json_value_free, &[child]),
                HOST_STATUS_SUCCESS
            );
            assert_eq!(
                call_json_host_no_result(json_value_free, &[root]),
                HOST_STATUS_SUCCESS
            );
            assert_eq!(
                call_json_host(json_value_kind, &[child]).1,
                JSON_KIND_INVALID,
                "freed child handles must classify as invalid"
            );
            assert_eq!(
                call_json_host(json_value_kind, &[root]).1,
                JSON_KIND_INVALID,
                "freed document handles must classify as invalid"
            );
        }
    }

    #[test]
    fn host_decode_field_by_key_extracts_scalars_and_nested() {
        let doc = alloc_spectra_string(r#"{"outer":{"x":5},"n":7}"#);
        let (_, root) = call_json_host(json_parse, &[doc]);
        assert_ne!(root, 0);

        // Scalar field: lookup by key and typed extraction in one call.
        let (_, n) = call_json_host(
            json_decode_field_by_key,
            &[
                root,
                alloc_spectra_string("n"),
                alloc_spectra_string("n"),
                alloc_spectra_string("int"),
                0,
                0,
            ],
        );
        assert_eq!(n, 7);

        // Absent optional field yields the default value.
        let (status, fallback) = call_json_host(
            json_decode_field_by_key,
            &[
                root,
                alloc_spectra_string("missing"),
                alloc_spectra_string("missing"),
                alloc_spectra_string("int"),
                1,
                42,
            ],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(fallback, 42);

        // Nested object mode returns a fresh handle the caller must free.
        let (status, nested) = call_json_host(
            json_decode_field_by_key,
            &[
                root,
                alloc_spectra_string("outer"),
                alloc_spectra_string("outer"),
                alloc_spectra_string("Outer"),
                0,
                0,
            ],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_ne!(nested, 0);
        assert_ne!(nested, root);
        let (_, x) = call_json_host(
            json_decode_field_by_key,
            &[
                nested,
                alloc_spectra_string("x"),
                alloc_spectra_string("outer.x"),
                alloc_spectra_string("int"),
                0,
                0,
            ],
        );
        assert_eq!(x, 5);
        assert_eq!(
            call_json_host_no_result(json_value_free, &[nested]),
            HOST_STATUS_SUCCESS
        );

        // Wrong wire type fails loudly instead of synthesizing a value.
        let (status, _) = call_json_host(
            json_decode_field_by_key,
            &[
                root,
                alloc_spectra_string("n"),
                alloc_spectra_string("n"),
                alloc_spectra_string("string"),
                0,
                0,
            ],
        );
        assert_eq!(status, HOST_STATUS_INVALID_ARGUMENT);

        assert_eq!(
            call_json_host_no_result(json_value_free, &[root]),
            HOST_STATUS_SUCCESS
        );
        assert_eq!(
            call_json_host(json_value_kind, &[root]).1,
            JSON_KIND_INVALID
        );
    }
}
