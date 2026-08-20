fn builtin_error_ir_type() -> IRType {
    IRType::Struct {
        name: "Error".to_string(),
        fields: vec![
            ("code".to_string(), IRType::Int),
            ("message".to_string(), IRType::String),
            ("operation".to_string(), IRType::String),
            ("context".to_string(), IRType::String),
            ("origin".to_string(), IRType::String),
            ("retryable".to_string(), IRType::Bool),
        ],
    }
}

fn builtin_result_ir_type(ok_type: IRType) -> IRType {
    let ok_name = match &ok_type {
        IRType::Int => "int",
        IRType::Bool => "bool",
        IRType::String => "string",
        IRType::Float => "float",
        IRType::Struct { name, .. } => name.as_str(),
        IRType::Void => "unit",
        _ => "unknown",
    };
    let error_type = builtin_error_ir_type();
    IRType::Enum {
        name: format!("Result_{ok_name}_Error"),
        variants: vec![
            ("Ok".to_string(), Some(vec![ok_type])),
            ("Err".to_string(), Some(vec![error_type])),
        ],
    }
}

