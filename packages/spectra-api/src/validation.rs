use crate::form::Form;
use crate::handles::ApiHandleTable;
use crate::http::{self, Response, Status};
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use regex::Regex;
use serde_json::{json, Value};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::fmt;
use std::sync::{Mutex, OnceLock};

pub const VALIDATION_TYPE_STRING: SpectraHostValue = 1;
pub const VALIDATION_TYPE_INT: SpectraHostValue = 2;
pub const VALIDATION_TYPE_BOOL: SpectraHostValue = 3;
pub const VALIDATION_TYPE_FLOAT: SpectraHostValue = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationValueType {
    String,
    Int,
    Bool,
    Float,
}

impl ValidationValueType {
    fn from_code(code: SpectraHostValue) -> Option<Self> {
        match code {
            VALIDATION_TYPE_STRING => Some(Self::String),
            VALIDATION_TYPE_INT => Some(Self::Int),
            VALIDATION_TYPE_BOOL => Some(Self::Bool),
            VALIDATION_TYPE_FLOAT => Some(Self::Float),
            _ => None,
        }
    }

    fn type_name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int => "integer",
            Self::Bool => "boolean",
            Self::Float => "number",
        }
    }
}

#[derive(Clone, Debug)]
struct Pattern {
    source: String,
    regex: Regex,
}

#[derive(Clone, Debug)]
pub struct ValidationField {
    pub name: String,
    pub value_type: ValidationValueType,
    pub required: bool,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pattern: Option<Pattern>,
}

#[derive(Clone, Debug, Default)]
pub struct ValidationSchema {
    fields: Vec<ValidationField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    InvalidSchema(String),
    UnknownField(String),
    InvalidConstraint(String),
}

impl ValidationError {
    fn code(&self) -> SpectraHostValue {
        match self {
            Self::InvalidSchema(_) => 1,
            Self::UnknownField(_) => 2,
            Self::InvalidConstraint(_) => 3,
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSchema(message) => write!(f, "invalid validation schema: {message}"),
            Self::UnknownField(field) => write!(f, "validation field {field:?} is not declared"),
            Self::InvalidConstraint(message) => {
                write!(f, "invalid validation constraint: {message}")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

impl ValidationSchema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fields(&self) -> &[ValidationField] {
        &self.fields
    }

    pub fn with_field(
        mut self,
        name: impl Into<String>,
        value_type: ValidationValueType,
        required: bool,
    ) -> Result<Self, ValidationError> {
        let name = name.into();
        if name.is_empty() {
            return Err(ValidationError::InvalidSchema(
                "validation field name cannot be empty".to_string(),
            ));
        }
        if self.fields.iter().any(|field| field.name == name) {
            return Err(ValidationError::InvalidSchema(format!(
                "duplicate validation field {name:?}"
            )));
        }
        self.fields.push(ValidationField {
            name,
            value_type,
            required,
            min_length: None,
            max_length: None,
            min_value: None,
            max_value: None,
            pattern: None,
        });
        Ok(self)
    }

    pub fn with_min_length(mut self, name: &str, value: usize) -> Result<Self, ValidationError> {
        let field = self.field_mut(name)?;
        if field.value_type != ValidationValueType::String {
            return Err(ValidationError::InvalidConstraint(format!(
                "min_length applies only to string field {name:?}"
            )));
        }
        if field.max_length.is_some_and(|max| value > max) {
            return Err(ValidationError::InvalidConstraint(format!(
                "min_length exceeds max_length for field {name:?}"
            )));
        }
        field.min_length = Some(value);
        Ok(self)
    }

    pub fn with_max_length(mut self, name: &str, value: usize) -> Result<Self, ValidationError> {
        let field = self.field_mut(name)?;
        if field.value_type != ValidationValueType::String {
            return Err(ValidationError::InvalidConstraint(format!(
                "max_length applies only to string field {name:?}"
            )));
        }
        if field.min_length.is_some_and(|min| value < min) {
            return Err(ValidationError::InvalidConstraint(format!(
                "max_length is below min_length for field {name:?}"
            )));
        }
        field.max_length = Some(value);
        Ok(self)
    }

    pub fn with_range(mut self, name: &str, min: i64, max: i64) -> Result<Self, ValidationError> {
        if min > max {
            return Err(ValidationError::InvalidConstraint(
                "range minimum cannot exceed maximum".to_string(),
            ));
        }
        let field = self.field_mut(name)?;
        if !matches!(
            field.value_type,
            ValidationValueType::Int | ValidationValueType::Float
        ) {
            return Err(ValidationError::InvalidConstraint(format!(
                "range applies only to numeric field {name:?}"
            )));
        }
        field.min_value = Some(min as f64);
        field.max_value = Some(max as f64);
        Ok(self)
    }

    pub fn with_regex(
        mut self,
        name: &str,
        pattern: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        let pattern = pattern.into();
        let regex = Regex::new(&pattern).map_err(|error| {
            ValidationError::InvalidConstraint(format!("invalid regex for field {name:?}: {error}"))
        })?;
        let field = self.field_mut(name)?;
        if field.value_type != ValidationValueType::String {
            return Err(ValidationError::InvalidConstraint(format!(
                "regex applies only to string field {name:?}"
            )));
        }
        field.pattern = Some(Pattern {
            source: pattern,
            regex,
        });
        Ok(self)
    }

    pub fn validate_json(&self, input: &str) -> ValidationResult {
        let value = match serde_json::from_str::<Value>(input) {
            Ok(value) => value,
            Err(error) => {
                return ValidationResult::invalid(vec![ValidationIssue::new(
                    "$",
                    "invalid_json",
                    format!("request body is not valid JSON: {error}"),
                )])
            }
        };
        let Some(object) = value.as_object() else {
            return ValidationResult::invalid(vec![ValidationIssue::new(
                "$",
                "object_required",
                "request JSON body must be an object",
            )]);
        };

        let mut issues = Vec::new();
        for field in &self.fields {
            let Some(value) = object.get(&field.name) else {
                if field.required {
                    issues.push(ValidationIssue::new(
                        &field.name,
                        "required",
                        "field is required",
                    ));
                }
                continue;
            };
            if let Some(issue) = validate_json_value(field, value) {
                issues.push(issue);
            }
        }
        ValidationResult::from_issues(issues)
    }

    pub fn validate_form(&self, form: &Form) -> ValidationResult {
        let mut issues = Vec::new();
        for field in &self.fields {
            let values = form.values(&field.name);
            let Some(value) = values.first() else {
                if field.required {
                    issues.push(ValidationIssue::new(
                        &field.name,
                        "required",
                        "field is required",
                    ));
                }
                continue;
            };
            if values.len() > 1 {
                issues.push(ValidationIssue::new(
                    &field.name,
                    "duplicate",
                    "field must occur only once",
                ));
                continue;
            }
            if let Some(issue) = validate_text_value(field, value) {
                issues.push(issue);
            }
        }
        ValidationResult::from_issues(issues)
    }

    fn field_mut(&mut self, name: &str) -> Result<&mut ValidationField, ValidationError> {
        self.fields
            .iter_mut()
            .find(|field| field.name == name)
            .ok_or_else(|| ValidationError::UnknownField(name.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationIssue {
    pub field: String,
    pub code: String,
    pub message: String,
}

impl ValidationIssue {
    fn new(field: impl Into<String>, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationResult {
    pub ok: bool,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    fn invalid(issues: Vec<ValidationIssue>) -> Self {
        Self { ok: false, issues }
    }

    fn from_issues(issues: Vec<ValidationIssue>) -> Self {
        Self {
            ok: issues.is_empty(),
            issues,
        }
    }

    pub fn problem_json(&self) -> String {
        let errors = self
            .issues
            .iter()
            .map(|issue| {
                json!({
                    "field": issue.field,
                    "code": issue.code,
                    "message": issue.message,
                })
            })
            .collect::<Vec<_>>();
        json!({
            "type": "https://spectra.dev/problems/validation",
            "title": if self.ok { "Request validation succeeded" } else { "Request validation failed" },
            "status": if self.ok { 204 } else { 422 },
            "detail": if self.ok { "request is valid" } else { "one or more request fields are invalid" },
            "errors": errors,
        })
        .to_string()
    }

    pub fn response(&self) -> Response {
        if self.ok {
            return Response::new(Status::new(204).expect("valid no-content status"));
        }
        Response::new(Status::new(422).expect("valid validation status"))
            .with_header("Content-Type", "application/problem+json")
            .expect("valid problem content type")
            .with_body(self.problem_json().into_bytes())
    }
}

fn validate_json_value(field: &ValidationField, value: &Value) -> Option<ValidationIssue> {
    match field.value_type {
        ValidationValueType::String => {
            let Some(value) = value.as_str() else {
                return Some(type_issue(field));
            };
            validate_string_constraints(field, value)
        }
        ValidationValueType::Int => {
            let Some(value) = value.as_i64() else {
                return Some(type_issue(field));
            };
            validate_number_constraints(field, value as f64)
        }
        ValidationValueType::Float => {
            let Some(value) = value.as_f64() else {
                return Some(type_issue(field));
            };
            validate_number_constraints(field, value)
        }
        ValidationValueType::Bool => value
            .as_bool()
            .map_or_else(|| Some(type_issue(field)), |_| None),
    }
}

fn validate_text_value(field: &ValidationField, value: &str) -> Option<ValidationIssue> {
    match field.value_type {
        ValidationValueType::String => validate_string_constraints(field, value),
        ValidationValueType::Int => match value.parse::<i64>() {
            Ok(number) => validate_number_constraints(field, number as f64),
            Err(_) => Some(type_issue(field)),
        },
        ValidationValueType::Float => match value.parse::<f64>() {
            Ok(number) => validate_number_constraints(field, number),
            Err(_) => Some(type_issue(field)),
        },
        ValidationValueType::Bool => match value {
            "true" | "false" | "1" | "0" | "on" | "off" | "yes" | "no" => None,
            _ => Some(type_issue(field)),
        },
    }
}

fn type_issue(field: &ValidationField) -> ValidationIssue {
    ValidationIssue::new(
        &field.name,
        "type",
        format!("field must be a {}", field.value_type.type_name()),
    )
}

fn validate_string_constraints(field: &ValidationField, value: &str) -> Option<ValidationIssue> {
    let length = value.chars().count();
    if field.min_length.is_some_and(|min| length < min) {
        return Some(ValidationIssue::new(
            &field.name,
            "min_length",
            format!(
                "field must contain at least {} characters",
                field.min_length.unwrap()
            ),
        ));
    }
    if field.max_length.is_some_and(|max| length > max) {
        return Some(ValidationIssue::new(
            &field.name,
            "max_length",
            format!(
                "field must contain at most {} characters",
                field.max_length.unwrap()
            ),
        ));
    }
    if let Some(pattern) = &field.pattern {
        if !pattern.regex.is_match(value) {
            return Some(ValidationIssue::new(
                &field.name,
                "regex",
                format!("field does not match pattern {:?}", pattern.source),
            ));
        }
    }
    None
}

fn validate_number_constraints(field: &ValidationField, value: f64) -> Option<ValidationIssue> {
    if field.min_value.is_some_and(|min| value < min) {
        return Some(ValidationIssue::new(
            &field.name,
            "min",
            format!("field must be at least {}", field.min_value.unwrap()),
        ));
    }
    if field.max_value.is_some_and(|max| value > max) {
        return Some(ValidationIssue::new(
            &field.name,
            "max",
            format!("field must be at most {}", field.max_value.unwrap()),
        ));
    }
    None
}

struct ValidationStore {
    schemas: ApiHandleTable<ValidationSchema>,
    results: ApiHandleTable<ValidationResult>,
    last_error_code: SpectraHostValue,
    last_error_message: String,
}

impl ValidationStore {
    fn new() -> Self {
        Self {
            schemas: ApiHandleTable::new(HandleKind::ApiValidationSchema),
            results: ApiHandleTable::new(HandleKind::ApiValidationResult),
            last_error_code: 0,
            last_error_message: String::new(),
        }
    }

    fn clear_error(&mut self) {
        self.last_error_code = 0;
        self.last_error_message.clear();
    }

    fn set_error(&mut self, error: ValidationError) {
        self.last_error_code = error.code();
        self.last_error_message = error.to_string();
    }
}

fn store() -> &'static Mutex<ValidationStore> {
    static STORE: OnceLock<Mutex<ValidationStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(ValidationStore::new()))
}

pub extern "C" fn schema(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store().lock().expect("validation store poisoned");
    store.clear_error();
    write_result(ctx, store.schemas.insert(ValidationSchema::new()))
}

pub extern "C" fn field(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 4) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(value_type) = ValidationValueType::from_code(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("validation store poisoned");
    let Some(existing) = store.schemas.get(&args[0]).cloned() else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match existing.with_field(name, value_type, args[3] != 0) {
        Ok(schema) => {
            store.clear_error();
            write_result(ctx, store.schemas.insert(schema))
        }
        Err(error) => {
            store.set_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn min_length(ctx: *mut SpectraHostCallContext) -> i32 {
    update_length_constraint(ctx, true)
}

pub extern "C" fn max_length(ctx: *mut SpectraHostCallContext) -> i32 {
    update_length_constraint(ctx, false)
}

fn update_length_constraint(ctx: *mut SpectraHostCallContext, minimum: bool) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(value) = usize::try_from(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("validation store poisoned");
    let Some(existing) = store.schemas.get(&args[0]).cloned() else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if !existing.fields.iter().any(|field| field.name == name) {
        store.set_error(ValidationError::UnknownField(name));
        return write_result(ctx, 0);
    }
    let result = if minimum {
        existing.with_min_length(&name, value)
    } else {
        existing.with_max_length(&name, value)
    };
    match result {
        Ok(schema) => {
            store.clear_error();
            write_result(ctx, store.schemas.insert(schema))
        }
        Err(error) => {
            store.set_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn range(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 4) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("validation store poisoned");
    let Some(existing) = store.schemas.get(&args[0]).cloned() else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match existing.with_range(&name, args[2], args[3]) {
        Ok(schema) => {
            store.clear_error();
            write_result(ctx, store.schemas.insert(schema))
        }
        Err(error) => {
            store.set_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn regex(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(name), Some(pattern)) = (read_spectra_string(args[1]), read_spectra_string(args[2]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("validation store poisoned");
    let Some(existing) = store.schemas.get(&args[0]).cloned() else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match existing.with_regex(&name, pattern) {
        Ok(schema) => {
            store.clear_error();
            write_result(ctx, store.schemas.insert(schema))
        }
        Err(error) => {
            store.set_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn validate_json(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(input) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("validation store poisoned");
    let Some(schema) = store.schemas.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let result = schema.validate_json(&input);
    store.clear_error();
    write_result(ctx, store.results.insert(result))
}

pub extern "C" fn validate_form(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("validation store poisoned");
    let Some(schema) = store.schemas.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(form) = crate::form::clone_form(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let result = schema.validate_form(&form);
    store.clear_error();
    write_result(ctx, store.results.insert(result))
}

pub extern "C" fn result_ok(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("validation store poisoned");
    let Some(result) = store.results.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, result.ok as SpectraHostValue)
}

pub extern "C" fn result_count(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("validation store poisoned");
    let Some(result) = store.results.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, result.issues.len() as SpectraHostValue)
}

pub extern "C" fn result_field(ctx: *mut SpectraHostCallContext) -> i32 {
    result_string(ctx, |issue| &issue.field)
}

pub extern "C" fn result_code(ctx: *mut SpectraHostCallContext) -> i32 {
    result_string(ctx, |issue| &issue.code)
}

pub extern "C" fn result_message(ctx: *mut SpectraHostCallContext) -> i32 {
    result_string(ctx, |issue| &issue.message)
}

fn result_string(
    ctx: *mut SpectraHostCallContext,
    select: impl Fn(&ValidationIssue) -> &str,
) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(index) = usize::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("validation store poisoned");
    let Some(result) = store.results.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(issue) = result.issues.get(index) else {
        return write_result(ctx, alloc_spectra_string(""));
    };
    write_result(ctx, alloc_spectra_string(select(issue)))
}

pub extern "C" fn result_problem_json(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("validation store poisoned");
    let Some(result) = store.results.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&result.problem_json()))
}

pub extern "C" fn result_response(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("validation store poisoned");
    let Some(result) = store.results.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, http::store_response(result.response()))
}

pub extern "C" fn error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(_) = read_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("validation store poisoned");
    write_result(ctx, store.last_error_code)
}

pub extern "C" fn error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(_) = read_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("validation store poisoned");
    write_result(ctx, alloc_spectra_string(&store.last_error_message))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> ValidationSchema {
        ValidationSchema::new()
            .with_field("name", ValidationValueType::String, true)
            .unwrap()
            .with_field("age", ValidationValueType::Int, true)
            .unwrap()
            .with_min_length("name", 3)
            .unwrap()
            .with_max_length("name", 20)
            .unwrap()
            .with_regex("name", r"^[A-Za-z]+$")
            .unwrap()
            .with_range("age", 18, 120)
            .unwrap()
    }

    #[test]
    fn json_validation_collects_constraints_and_rfc7807_errors() {
        let result = schema().validate_json(r#"{"name":"A1","age":12}"#);
        assert!(!result.ok);
        assert_eq!(result.issues.len(), 2);
        assert_eq!(result.issues[0].code, "min_length");
        assert_eq!(result.issues[1].code, "min");
        let problem: Value = serde_json::from_str(&result.problem_json()).expect("problem JSON");
        assert_eq!(problem["type"], "https://spectra.dev/problems/validation");
        assert_eq!(problem["status"], 422);
        assert_eq!(problem["errors"].as_array().unwrap().len(), 2);
        assert_eq!(result.response().status.code(), 422);
        assert_eq!(
            result.response().header("content-type"),
            Some("application/problem+json")
        );
    }

    #[test]
    fn form_and_json_validation_accept_matching_values() {
        let schema = schema();
        let json = schema.validate_json(r#"{"name":"Ada","age":37}"#);
        assert!(json.ok);
        assert!(json.issues.is_empty());
        let form = Form::parse("name=Ada&age=37").expect("form");
        let form_result = schema.validate_form(&form);
        assert!(form_result.ok);
        assert_eq!(form_result.response().status.code(), 204);
    }

    #[test]
    fn invalid_schema_constraints_are_rejected_before_validation() {
        let schema = ValidationSchema::new()
            .with_field("age", ValidationValueType::Int, true)
            .expect("field");
        assert!(matches!(
            schema.clone().with_regex("age", "digits"),
            Err(ValidationError::InvalidConstraint(_))
        ));
        assert!(matches!(
            schema.with_range("age", 20, 10),
            Err(ValidationError::InvalidConstraint(_))
        ));
    }
}
