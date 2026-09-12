use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use crate::handles::ApiHandleTable;
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Mutex, OnceLock};

pub const FORM_TYPE_STRING: SpectraHostValue = 1;
pub const FORM_TYPE_INT: SpectraHostValue = 2;
pub const FORM_TYPE_BOOL: SpectraHostValue = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pairs: Vec<(String, String)>,
    values: BTreeMap<String, Vec<String>>,
}

impl Form {
    pub fn parse(input: &str) -> Result<Self, FormParseError> {
        parse_form(input)
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

    pub fn int(&self, key: &str, index: usize) -> Result<i64, FormBindError> {
        let Some(value) = self.get(key, index) else {
            return Err(FormBindError::MissingField(key.to_string()));
        };
        coerce_int(key, value)
    }

    pub fn bool(&self, key: &str, index: usize) -> Result<bool, FormBindError> {
        let Some(value) = self.get(key, index) else {
            return Err(FormBindError::MissingField(key.to_string()));
        };
        coerce_bool(key, value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormParseErrorKind {
    InvalidPercentEncoding,
    InvalidUtf8,
    EmptyKey,
    ControlCharacter,
    MalformedKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormParseError {
    pub kind: FormParseErrorKind,
    pub position: usize,
    pub field: String,
    pub message: String,
}

impl FormParseError {
    fn new(
        kind: FormParseErrorKind,
        position: usize,
        field: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            position,
            field: field.into(),
            message: message.into(),
        }
    }

    fn code(&self) -> SpectraHostValue {
        match self.kind {
            FormParseErrorKind::InvalidPercentEncoding => 1,
            FormParseErrorKind::InvalidUtf8 => 2,
            FormParseErrorKind::EmptyKey => 3,
            FormParseErrorKind::ControlCharacter => 4,
            FormParseErrorKind::MalformedKey => 5,
        }
    }
}

impl fmt::Display for FormParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            write!(f, "{} at byte {}", self.message, self.position)
        } else {
            write!(
                f,
                "{} for field {:?} at byte {}",
                self.message, self.field, self.position
            )
        }
    }
}

impl std::error::Error for FormParseError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormValueType {
    String,
    Int,
    Bool,
}

impl FormValueType {
    fn from_code(code: SpectraHostValue) -> Option<Self> {
        match code {
            FORM_TYPE_STRING => Some(Self::String),
            FORM_TYPE_INT => Some(Self::Int),
            FORM_TYPE_BOOL => Some(Self::Bool),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormFieldSchema {
    pub name: String,
    pub value_type: FormValueType,
    pub required: bool,
    pub repeated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormSchema {
    fields: Vec<FormFieldSchema>,
}

impl FormSchema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(
        mut self,
        name: impl Into<String>,
        value_type: FormValueType,
        required: bool,
        repeated: bool,
    ) -> Result<Self, FormBindError> {
        let name = name.into();
        if name.is_empty() {
            return Err(FormBindError::InvalidSchema(
                "form schema field name cannot be empty".to_string(),
            ));
        }
        if self.fields.iter().any(|field| field.name == name) {
            return Err(FormBindError::InvalidSchema(format!(
                "duplicate form schema field {name:?}"
            )));
        }
        self.fields.push(FormFieldSchema {
            name,
            value_type,
            required,
            repeated,
        });
        Ok(self)
    }

    pub fn bind(&self, form: &Form) -> FormBinding {
        let mut values = HashMap::new();
        for field in &self.fields {
            let raw_values = form.values(&field.name);
            if raw_values.is_empty() {
                if field.required {
                    return FormBinding::error(FormBindError::MissingField(field.name.clone()));
                }
                values.insert(field.name.clone(), Vec::new());
                continue;
            }
            if !field.repeated && raw_values.len() > 1 {
                return FormBinding::error(FormBindError::DuplicateField(field.name.clone()));
            }
            for value in raw_values {
                if let Err(error) = validate_coercion(field, value) {
                    return FormBinding::error(error);
                }
            }
            values.insert(field.name.clone(), raw_values.to_vec());
        }
        FormBinding {
            ok: true,
            values,
            error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormBindError {
    InvalidSchema(String),
    MissingField(String),
    DuplicateField(String),
    InvalidInt { field: String, value: String },
    InvalidBool { field: String, value: String },
}

impl FormBindError {
    fn code(&self) -> SpectraHostValue {
        match self {
            Self::InvalidSchema(_) => 10,
            Self::MissingField(_) => 11,
            Self::DuplicateField(_) => 12,
            Self::InvalidInt { .. } => 13,
            Self::InvalidBool { .. } => 14,
        }
    }
}

impl fmt::Display for FormBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSchema(message) => write!(f, "invalid form schema: {message}"),
            Self::MissingField(field) => write!(f, "missing required form field {field:?}"),
            Self::DuplicateField(field) => write!(f, "duplicate scalar form field {field:?}"),
            Self::InvalidInt { field, value } => {
                write!(f, "form field {field:?} expected int, got {value:?}")
            }
            Self::InvalidBool { field, value } => {
                write!(f, "form field {field:?} expected bool, got {value:?}")
            }
        }
    }
}

impl std::error::Error for FormBindError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormBinding {
    ok: bool,
    values: HashMap<String, Vec<String>>,
    error: Option<FormBindError>,
}

impl FormBinding {
    fn error(error: FormBindError) -> Self {
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

    pub fn int(&self, field: &str, index: usize) -> Result<i64, FormBindError> {
        let Some(value) = self.get(field, index) else {
            return Err(FormBindError::MissingField(field.to_string()));
        };
        coerce_int(field, value)
    }

    pub fn bool(&self, field: &str, index: usize) -> Result<bool, FormBindError> {
        let Some(value) = self.get(field, index) else {
            return Err(FormBindError::MissingField(field.to_string()));
        };
        coerce_bool(field, value)
    }
}

struct FormStore {
    forms: ApiHandleTable<Form>,
    schemas: ApiHandleTable<FormSchema>,
    bindings: ApiHandleTable<FormBinding>,
    last_error_code: SpectraHostValue,
    last_error_message: String,
}

impl FormStore {
    fn new() -> Self {
        Self {
            forms: ApiHandleTable::new(HandleKind::ApiForm),
            schemas: ApiHandleTable::new(HandleKind::ApiFormSchema),
            bindings: ApiHandleTable::new(HandleKind::ApiFormBinding),
            last_error_code: 0,
            last_error_message: String::new(),
        }
    }

    fn form_handle(&mut self, form: Form) -> SpectraHostValue {
        self.forms.insert(form)
    }

    fn schema_handle(&mut self, schema: FormSchema) -> SpectraHostValue {
        self.schemas.insert(schema)
    }

    fn binding_handle(&mut self, binding: FormBinding) -> SpectraHostValue {
        self.bindings.insert(binding)
    }

    fn clear_error(&mut self) {
        self.last_error_code = 0;
        self.last_error_message.clear();
    }

    fn set_parse_error(&mut self, error: FormParseError) {
        self.last_error_code = error.code();
        self.last_error_message = error.to_string();
    }

    fn set_bind_error(&mut self, error: FormBindError) {
        self.last_error_code = error.code();
        self.last_error_message = error.to_string();
    }
}

fn store() -> &'static Mutex<FormStore> {
    static STORE: OnceLock<Mutex<FormStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(FormStore::new()))
}

pub(crate) fn clone_form(handle: SpectraHostValue) -> Option<Form> {
    let store = store().lock().expect("form store poisoned");
    store.forms.get(&handle).cloned()
}

fn parse_form(input: &str) -> Result<Form, FormParseError> {
    if input.is_empty() {
        return Ok(Form {
            pairs: Vec::new(),
            values: BTreeMap::new(),
        });
    }
    let mut pairs = Vec::new();
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pair_start = 0usize;
    for raw_pair in input.split('&') {
        if raw_pair.is_empty() {
            pair_start += 1;
            continue;
        }
        let equal_offset = raw_pair.find('=');
        let (raw_key, raw_value) = match equal_offset {
            Some(offset) => (&raw_pair[..offset], &raw_pair[offset + 1..]),
            None => (raw_pair, ""),
        };
        let decoded_key = form_decode(raw_key, pair_start)?;
        if decoded_key.is_empty() {
            return Err(FormParseError::new(
                FormParseErrorKind::EmptyKey,
                pair_start,
                "",
                "form field name cannot be empty",
            ));
        }
        let normalized_key = normalize_form_key(&decoded_key, pair_start)?;
        let value_start = equal_offset
            .map(|offset| pair_start + offset + 1)
            .unwrap_or(pair_start);
        let decoded_value = form_decode(raw_value, value_start)?;
        pairs.push((normalized_key.clone(), decoded_value.clone()));
        values
            .entry(normalized_key)
            .or_default()
            .push(decoded_value);
        pair_start += raw_pair.len() + 1;
    }
    Ok(Form { pairs, values })
}

fn form_decode(input: &str, base_position: usize) -> Result<String, FormParseError> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(FormParseError::new(
                        FormParseErrorKind::InvalidPercentEncoding,
                        base_position + index,
                        "",
                        "percent encoding must contain two hex digits",
                    ));
                }
                let hi = hex_value(bytes[index + 1]);
                let lo = hex_value(bytes[index + 2]);
                let (Some(hi), Some(lo)) = (hi, lo) else {
                    return Err(FormParseError::new(
                        FormParseErrorKind::InvalidPercentEncoding,
                        base_position + index,
                        "",
                        "percent encoding contains a non-hex digit",
                    ));
                };
                decoded.push((hi << 4) | lo);
                index += 3;
            }
            byte if byte < 0x20 || byte == 0x7f => {
                return Err(FormParseError::new(
                    FormParseErrorKind::ControlCharacter,
                    base_position + index,
                    "",
                    "form bodies cannot contain raw control characters",
                ));
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| {
        FormParseError::new(
            FormParseErrorKind::InvalidUtf8,
            base_position,
            "",
            "percent decoded form component is not valid UTF-8",
        )
    })
}

fn normalize_form_key(key: &str, position: usize) -> Result<String, FormParseError> {
    if key.is_empty() || key.starts_with('[') {
        return Err(FormParseError::new(
            FormParseErrorKind::EmptyKey,
            position,
            key,
            "form field name cannot be empty",
        ));
    }
    if !key.contains('[') {
        return Ok(key.to_string());
    }

    let mut normalized = String::new();
    let mut chars = key.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '[' => {
                let mut segment = String::new();
                let mut closed = false;
                for (_, inner) in chars.by_ref() {
                    if inner == ']' {
                        closed = true;
                        break;
                    }
                    if inner == '[' {
                        return Err(FormParseError::new(
                            FormParseErrorKind::MalformedKey,
                            position + idx,
                            key,
                            "nested form key segment opened before closing the previous segment",
                        ));
                    }
                    segment.push(inner);
                }
                if !closed {
                    return Err(FormParseError::new(
                        FormParseErrorKind::MalformedKey,
                        position + idx,
                        key,
                        "form key is missing a closing bracket",
                    ));
                }
                if !segment.is_empty() {
                    if normalized.is_empty() {
                        return Err(FormParseError::new(
                            FormParseErrorKind::MalformedKey,
                            position + idx,
                            key,
                            "form key cannot start with a nested segment",
                        ));
                    }
                    normalized.push('.');
                    normalized.push_str(&segment);
                }
            }
            ']' => {
                return Err(FormParseError::new(
                    FormParseErrorKind::MalformedKey,
                    position + idx,
                    key,
                    "form key contains an unmatched closing bracket",
                ));
            }
            _ => normalized.push(ch),
        }
    }
    if normalized.is_empty() {
        return Err(FormParseError::new(
            FormParseErrorKind::EmptyKey,
            position,
            key,
            "form field name cannot be empty",
        ));
    }
    Ok(normalized)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn validate_coercion(field: &FormFieldSchema, value: &str) -> Result<(), FormBindError> {
    match field.value_type {
        FormValueType::String => Ok(()),
        FormValueType::Int => coerce_int(&field.name, value).map(|_| ()),
        FormValueType::Bool => coerce_bool(&field.name, value).map(|_| ()),
    }
}

fn coerce_int(field: &str, value: &str) -> Result<i64, FormBindError> {
    value.parse::<i64>().map_err(|_| FormBindError::InvalidInt {
        field: field.to_string(),
        value: value.to_string(),
    })
}

fn coerce_bool(field: &str, value: &str) -> Result<bool, FormBindError> {
    match value {
        "true" | "1" | "on" | "yes" => Ok(true),
        "false" | "0" | "off" | "no" => Ok(false),
        _ => Err(FormBindError::InvalidBool {
            field: field.to_string(),
            value: value.to_string(),
        }),
    }
}

pub extern "C" fn type_string(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, FORM_TYPE_STRING)
}

pub extern "C" fn type_int(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, FORM_TYPE_INT)
}

pub extern "C" fn type_bool(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, FORM_TYPE_BOOL)
}

pub extern "C" fn parse(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(input) = read_spectra_string(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("form store poisoned");
    match Form::parse(&input) {
        Ok(form) => {
            store.clear_error();
            let handle = store.form_handle(form);
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
    let store = store().lock().expect("form store poisoned");
    let len = store.forms.get(&args[0]).map(Form::len).unwrap_or(0);
    write_result(ctx, len as SpectraHostValue)
}

pub extern "C" fn has(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("form store poisoned");
    let has = store
        .forms
        .get(&args[0])
        .map(|form| form.has(&key))
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
    let store = store().lock().expect("form store poisoned");
    let count = store
        .forms
        .get(&args[0])
        .map(|form| form.values(&key).len())
        .unwrap_or(0);
    write_result(ctx, count as SpectraHostValue)
}

pub extern "C" fn first(ctx: *mut SpectraHostCallContext) -> i32 {
    form_string_lookup(ctx, |form, key| form.first(key).map(str::to_string))
}

pub extern "C" fn value(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[2].max(0) as usize;
    let store = store().lock().expect("form store poisoned");
    let value = store
        .forms
        .get(&args[0])
        .and_then(|form| form.get(&key, index))
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
    let mut store = store().lock().expect("form store poisoned");
    let result = store
        .forms
        .get(&args[0])
        .ok_or_else(|| FormBindError::MissingField(key.clone()))
        .and_then(|form| form.int(&key, index));
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
    let mut store = store().lock().expect("form store poisoned");
    let result = store
        .forms
        .get(&args[0])
        .ok_or_else(|| FormBindError::MissingField(key.clone()))
        .and_then(|form| form.bool(&key, index));
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
    let mut store = store().lock().expect("form store poisoned");
    store.clear_error();
    let handle = store.schema_handle(FormSchema::new());
    write_result(ctx, handle)
}

pub extern "C" fn schema_field(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(value_type) = FormValueType::from_code(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let required = args[3] != 0;
    let repeated = args[4] != 0;
    let mut store = store().lock().expect("form store poisoned");
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
    let mut store = store().lock().expect("form store poisoned");
    let binding = {
        let Some(form) = store.forms.get(&args[0]) else {
            let error = FormBindError::MissingField("<form>".to_string());
            store.set_bind_error(error.clone());
            let handle = store.binding_handle(FormBinding::error(error));
            return write_result(ctx, handle);
        };
        let Some(schema) = store.schemas.get(&args[1]) else {
            let error = FormBindError::InvalidSchema("schema handle not found".to_string());
            store.set_bind_error(error.clone());
            let handle = store.binding_handle(FormBinding::error(error));
            return write_result(ctx, handle);
        };
        schema.bind(form)
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
    let store = store().lock().expect("form store poisoned");
    let ok = store
        .bindings
        .get(&args[0])
        .map(FormBinding::ok)
        .unwrap_or(false);
    write_result(ctx, i64::from(ok))
}

pub extern "C" fn binding_error(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("form store poisoned");
    let message = store
        .bindings
        .get(&args[0])
        .map(FormBinding::error_message)
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
    let store = store().lock().expect("form store poisoned");
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
    let store = store().lock().expect("form store poisoned");
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
    let mut store = store().lock().expect("form store poisoned");
    let result = store
        .bindings
        .get(&args[0])
        .ok_or_else(|| FormBindError::MissingField(field.clone()))
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
    let store = store().lock().expect("form store poisoned");
    write_result(ctx, store.last_error_code)
}

pub extern "C" fn error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let store = store().lock().expect("form store poisoned");
    write_result(ctx, alloc_spectra_string(&store.last_error_message))
}

fn form_string_lookup(
    ctx: *mut SpectraHostCallContext,
    lookup: impl FnOnce(&Form, &str) -> Option<String>,
) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("form store poisoned");
    let value = store
        .forms
        .get(&args[0])
        .and_then(|form| lookup(form, &key))
        .unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&value))
}

fn binding_int_lookup(
    ctx: *mut SpectraHostCallContext,
    lookup: impl FnOnce(&FormBinding, &str, usize) -> Result<i64, FormBindError>,
) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(field) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[2].max(0) as usize;
    let mut store = store().lock().expect("form store poisoned");
    let result = store
        .bindings
        .get(&args[0])
        .ok_or_else(|| FormBindError::MissingField(field.clone()))
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
