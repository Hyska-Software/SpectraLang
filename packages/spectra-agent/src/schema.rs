//! Client-side JSON Schema validation for `ask_json` (R-3211 T3).
//!
//! A provider that claims schema-constrained decoding is not trusted: the
//! response is validated here, independently of the provider, before it
//! reaches compiled code. The supported keyword subset is deliberately small
//! and documented; unknown keywords are ignored, as JSON Schema requires.

use serde_json::Value;

/// Validates `instance_json` against `schema_json`.
///
/// Returns the first violation as `<path>: <detail>`; the caller turns it into
/// a typed `schema_violation` error.
pub(crate) fn validate(instance_json: &str, schema_json: &str) -> Result<(), String> {
    let instance: Value = serde_json::from_str(instance_json)
        .map_err(|error| format!("$: response is not valid JSON: {error}"))?;
    let schema: Value = serde_json::from_str(schema_json)
        .map_err(|error| format!("$: schema is not valid JSON: {error}"))?;
    validate_value(&instance, &schema, "$")
}

fn validate_value(instance: &Value, schema: &Value, path: &str) -> Result<(), String> {
    let Some(object) = schema.as_object() else {
        // A boolean schema is valid JSON Schema (`true` accepts anything).
        return match schema {
            Value::Bool(true) | Value::Object(_) => Ok(()),
            Value::Bool(false) => Err(format!("{path}: rejected by the false schema")),
            _ => Ok(()),
        };
    };

    if let Some(expected) = object.get("type") {
        let matches = match expected {
            Value::String(name) => type_matches(instance, name),
            Value::Array(names) => names
                .iter()
                .filter_map(Value::as_str)
                .any(|name| type_matches(instance, name)),
            _ => true,
        };
        if !matches {
            return Err(format!(
                "{path}: expected type {expected}, found {}",
                instance_type(instance)
            ));
        }
    }

    if let Some(allowed) = object.get("enum").and_then(Value::as_array) {
        if !allowed.contains(instance) {
            return Err(format!("{path}: value is not one of the enum members"));
        }
    }
    if let Some(constant) = object.get("const") {
        if constant != instance {
            return Err(format!("{path}: value does not equal the declared const"));
        }
    }

    if let Some(Value::Number(minimum)) = object.get("minimum") {
        if let (Some(value), Some(minimum)) = (instance.as_f64(), minimum.as_f64()) {
            if value < minimum {
                return Err(format!("{path}: {value} is below minimum {minimum}"));
            }
        }
    }
    if let Some(Value::Number(maximum)) = object.get("maximum") {
        if let (Some(value), Some(maximum)) = (instance.as_f64(), maximum.as_f64()) {
            if value > maximum {
                return Err(format!("{path}: {value} is above maximum {maximum}"));
            }
        }
    }

    if let Some(text) = instance.as_str() {
        let length = text.chars().count() as u64;
        if let Some(minimum) = object.get("minLength").and_then(Value::as_u64) {
            if length < minimum {
                return Err(format!("{path}: string is shorter than minLength {minimum}"));
            }
        }
        if let Some(maximum) = object.get("maxLength").and_then(Value::as_u64) {
            if length > maximum {
                return Err(format!("{path}: string is longer than maxLength {maximum}"));
            }
        }
    }

    if let Some(array) = instance.as_array() {
        let length = array.len() as u64;
        if let Some(minimum) = object.get("minItems").and_then(Value::as_u64) {
            if length < minimum {
                return Err(format!("{path}: array has fewer than minItems {minimum}"));
            }
        }
        if let Some(maximum) = object.get("maxItems").and_then(Value::as_u64) {
            if length > maximum {
                return Err(format!("{path}: array has more than maxItems {maximum}"));
            }
        }
        if let Some(items) = object.get("items") {
            for (index, element) in array.iter().enumerate() {
                validate_value(element, items, &format!("{path}[{index}]"))?;
            }
        }
    }

    if let Some(instance_object) = instance.as_object() {
        let properties = object.get("properties").and_then(Value::as_object);
        if let Some(required) = object.get("required").and_then(Value::as_array) {
            for name in required.iter().filter_map(Value::as_str) {
                if !instance_object.contains_key(name) {
                    return Err(format!("{path}: required property '{name}' is missing"));
                }
            }
        }
        if let Some(properties) = properties {
            for (name, subschema) in properties {
                if let Some(value) = instance_object.get(name) {
                    validate_value(value, subschema, &format!("{path}.{name}"))?;
                }
            }
        }
        if object.get("additionalProperties") == Some(&Value::Bool(false)) {
            for name in instance_object.keys() {
                let known = properties.map(|p| p.contains_key(name)).unwrap_or(false);
                if !known {
                    return Err(format!(
                        "{path}: additional property '{name}' is not allowed"
                    ));
                }
            }
        }
    }

    Ok(())
}

fn type_matches(instance: &Value, expected: &str) -> bool {
    match expected {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "number" => instance.is_number(),
        "integer" => instance
            .as_f64()
            .map(|value| value.is_finite() && value.fract() == 0.0)
            .unwrap_or(false),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        _ => true,
    }
}

fn instance_type(instance: &Value) -> &'static str {
    match instance {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r#"{
        "type": "object",
        "properties": {
            "count": { "type": "integer", "minimum": 0 },
            "label": { "type": "string", "minLength": 1 }
        },
        "required": ["count", "label"],
        "additionalProperties": false
    }"#;

    #[test]
    fn accepts_a_conforming_document() {
        assert_eq!(validate(r#"{"count":3,"label":"mock"}"#, SCHEMA), Ok(()));
        assert_eq!(validate(r#"{"count":0,"label":"x"}"#, SCHEMA), Ok(()));
    }

    #[test]
    fn rejects_each_violation_with_a_path() {
        let wrong_type = validate(r#"{"count":"3","label":"x"}"#, SCHEMA).expect_err("type");
        assert!(wrong_type.contains("$.count"), "{wrong_type}");

        let missing = validate(r#"{"label":"x"}"#, SCHEMA).expect_err("required");
        assert!(missing.contains("required property 'count'"), "{missing}");

        let extra = validate(r#"{"count":1,"label":"x","extra":true}"#, SCHEMA)
            .expect_err("additional");
        assert!(extra.contains("'extra'"), "{extra}");

        let negative = validate(r#"{"count":-1,"label":"x"}"#, SCHEMA).expect_err("minimum");
        assert!(negative.contains("below minimum"), "{negative}");

        let short = validate(r#"{"count":1,"label":""}"#, SCHEMA).expect_err("minLength");
        assert!(short.contains("minLength"), "{short}");
    }

    #[test]
    fn invalid_json_is_reported_before_the_schema_is_consulted() {
        assert!(validate("not json", SCHEMA)
            .expect_err("bad instance")
            .contains("not valid JSON"));
        assert!(validate("{}", "not json")
            .expect_err("bad schema")
            .contains("schema is not valid JSON"));
    }

    #[test]
    fn supports_arrays_and_nested_objects() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "rows": {
                    "type": "array",
                    "minItems": 1,
                    "items": { "type": "object", "properties": { "n": { "type": "integer" } }, "required": ["n"] }
                }
            },
            "required": ["rows"]
        }"#;
        assert_eq!(validate(r#"{"rows":[{"n":1},{"n":2}]}"#, schema), Ok(()));
        let error = validate(r#"{"rows":[{"n":1},{"n":"x"}]}"#, schema).expect_err("nested");
        assert!(error.contains("$.rows[1].n"), "{error}");
        let empty = validate(r#"{"rows":[]}"#, schema).expect_err("minItems");
        assert!(empty.contains("minItems"), "{empty}");
    }

    #[test]
    fn integer_type_accepts_integral_floats_only() {
        assert_eq!(validate("3", r#"{"type":"integer"}"#), Ok(()));
        assert_eq!(validate("3.0", r#"{"type":"integer"}"#), Ok(()));
        assert!(validate("3.5", r#"{"type":"integer"}"#).is_err());
    }
}
