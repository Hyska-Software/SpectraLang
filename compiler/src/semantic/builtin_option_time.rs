use super::*;
use crate::ast::Type;
use crate::semantic::module_registry::ModuleExports;

pub(crate) fn make_std_option() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "option".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let option = Type::Enum {
        name: "Option".to_string(),
    };
    let payload = Type::TypeParameter {
        name: "T".to_string(),
    };

    // `Option` is represented by a specialized enum at use sites. The bare
    // enum name here is a typed generic pattern, not an unknown wildcard.
    exports.functions.insert(
        "is_some".to_string(),
        pub_fn(vec![option.clone()], Type::Bool),
    );
    exports.functions.insert(
        "is_none".to_string(),
        pub_fn(vec![option.clone()], Type::Bool),
    );
    exports.functions.insert(
        "option_unwrap".to_string(),
        pub_fn(vec![option.clone()], payload.clone()),
    );
    exports.functions.insert(
        "option_unwrap_or".to_string(),
        pub_fn(vec![option, payload.clone()], payload),
    );
    let mapped = Type::TypeParameter {
        name: "U".to_string(),
    };
    exports.functions.insert(
        "option_map".to_string(),
        pub_fn(
            vec![
                Type::Enum {
                    name: "Option".to_string(),
                },
                Type::Fn {
                    params: vec![Type::TypeParameter {
                        name: "T".to_string(),
                    }],
                    return_type: Box::new(mapped),
                },
            ],
            Type::Applied {
                name: "Option".to_string(),
                // `U` is instantiated from the closure's return type at each
                // call site by `specialize_std_collection_signature`.
                args: vec![Type::TypeParameter {
                    name: "U".to_string(),
                }],
            },
        ),
    );

    exports
}

pub(crate) fn make_std_result() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "result".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let result = Type::Enum {
        name: "Result".to_string(),
    };
    let value = Type::TypeParameter {
        name: "T".to_string(),
    };
    let error = Type::TypeParameter {
        name: "E".to_string(),
    };

    exports.functions.insert(
        "is_ok".to_string(),
        pub_fn(vec![result.clone()], Type::Bool),
    );
    exports.functions.insert(
        "is_err".to_string(),
        pub_fn(vec![result.clone()], Type::Bool),
    );
    exports.functions.insert(
        "result_unwrap".to_string(),
        pub_fn(vec![result.clone()], value.clone()),
    );
    exports.functions.insert(
        "result_unwrap_or".to_string(),
        pub_fn(vec![result.clone(), value.clone()], value),
    );
    exports
        .functions
        .insert("result_unwrap_err".to_string(), pub_fn(vec![result], error));
    let mapped = Type::TypeParameter {
        name: "U".to_string(),
    };
    exports.functions.insert(
        "result_map".to_string(),
        pub_fn(
            vec![
                Type::Enum {
                    name: "Result".to_string(),
                },
                Type::Fn {
                    params: vec![Type::TypeParameter {
                        name: "T".to_string(),
                    }],
                    return_type: Box::new(mapped.clone()),
                },
            ],
            Type::Applied {
                name: "Result".to_string(),
                // `U` comes from the closure's return type; the error side
                // keeps the input's `E`.
                args: vec![
                    Type::TypeParameter {
                        name: "U".to_string(),
                    },
                    Type::TypeParameter {
                        name: "E".to_string(),
                    },
                ],
            },
        ),
    );
    exports.functions.insert(
        "result_map_err".to_string(),
        pub_fn(
            vec![
                Type::Enum {
                    name: "Result".to_string(),
                },
                Type::Fn {
                    params: vec![Type::TypeParameter {
                        name: "E".to_string(),
                    }],
                    return_type: Box::new(mapped),
                },
            ],
            Type::Applied {
                name: "Result".to_string(),
                // The value side keeps the input's `T`; `U` comes from the
                // closure's return type.
                args: vec![
                    Type::TypeParameter {
                        name: "T".to_string(),
                    },
                    Type::TypeParameter {
                        name: "U".to_string(),
                    },
                ],
            },
        ),
    );

    exports
}

pub(crate) fn make_std_char() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "char".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    // All functions take an int (Unicode code point) and return bool or int.
    // is_alpha(c: int) -> bool
    exports
        .functions
        .insert("is_alpha".to_string(), pub_fn(vec![Type::Int], Type::Bool));
    // is_digit_char(c: int) -> bool
    exports.functions.insert(
        "is_digit_char".to_string(),
        pub_fn(vec![Type::Int], Type::Bool),
    );
    // is_whitespace_char(c: int) -> bool
    exports.functions.insert(
        "is_whitespace_char".to_string(),
        pub_fn(vec![Type::Int], Type::Bool),
    );
    // is_upper_char(c: int) -> bool
    exports.functions.insert(
        "is_upper_char".to_string(),
        pub_fn(vec![Type::Int], Type::Bool),
    );
    // is_lower_char(c: int) -> bool
    exports.functions.insert(
        "is_lower_char".to_string(),
        pub_fn(vec![Type::Int], Type::Bool),
    );
    // to_upper_char(c: int) -> int  (returns uppercased code point)
    exports.functions.insert(
        "to_upper_char".to_string(),
        pub_fn(vec![Type::Int], Type::Int),
    );
    // to_lower_char(c: int) -> int  (returns lowercased code point)
    exports.functions.insert(
        "to_lower_char".to_string(),
        pub_fn(vec![Type::Int], Type::Int),
    );
    // is_alphanumeric(c: int) -> bool
    exports.functions.insert(
        "is_alphanumeric".to_string(),
        pub_fn(vec![Type::Int], Type::Bool),
    );

    exports
}

pub(crate) fn make_std_time() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "time".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    for name in ["Duration", "Instant", "UtcDateTime"] {
        exports.types.insert(name.to_string(), public_type(&[]));
    }

    let duration = Type::Struct {
        name: "Duration".to_string(),
    };
    let instant = Type::Struct {
        name: "Instant".to_string(),
    };
    let utc = Type::Struct {
        name: "UtcDateTime".to_string(),
    };

    // time_now_millis() -> int  (milliseconds since Unix epoch; -1 on error)
    exports
        .functions
        .insert("time_now_millis".to_string(), pub_fn(vec![], Type::Int));
    // time_now_secs() -> int  (seconds since Unix epoch; -1 on error)
    exports
        .functions
        .insert("time_now_secs".to_string(), pub_fn(vec![], Type::Int));
    // sleep_ms(ms: int) -> unit  (sleeps for ms milliseconds)
    exports
        .functions
        .insert("sleep_ms".to_string(), pub_fn(vec![Type::Int], Type::Unit));
    exports
        .functions
        .insert("monotonic_millis".to_string(), pub_fn(vec![], Type::Int));
    exports
        .functions
        .insert("monotonic_nanos".to_string(), pub_fn(vec![], Type::Int));
    exports.functions.insert(
        "duration_ms".to_string(),
        pub_fn(vec![Type::Int], duration.clone()),
    );
    exports.functions.insert(
        "duration_secs".to_string(),
        pub_fn(vec![Type::Int], duration.clone()),
    );
    exports.functions.insert(
        "duration_millis".to_string(),
        pub_fn(vec![duration.clone()], Type::Int),
    );
    exports.functions.insert(
        "duration_secs_value".to_string(),
        pub_fn(vec![duration.clone()], Type::Int),
    );
    exports.functions.insert(
        "duration_add".to_string(),
        pub_fn(vec![duration.clone(), duration.clone()], duration.clone()),
    );
    exports.functions.insert(
        "duration_sub".to_string(),
        pub_fn(vec![duration.clone(), duration.clone()], duration.clone()),
    );
    exports
        .functions
        .insert("instant_now".to_string(), pub_fn(vec![], instant.clone()));
    exports.functions.insert(
        "instant_elapsed_ms".to_string(),
        pub_fn(vec![instant.clone()], Type::Int),
    );
    exports.functions.insert(
        "instant_add".to_string(),
        pub_fn(vec![instant.clone(), duration.clone()], instant.clone()),
    );
    exports.functions.insert(
        "instant_has_elapsed".to_string(),
        pub_fn(vec![instant.clone()], Type::Bool),
    );
    exports
        .functions
        .insert("sleep".to_string(), pub_fn(vec![duration], Type::Unit));
    exports.functions.insert(
        "unix_to_utc".to_string(),
        pub_fn(vec![Type::Int], utc.clone()),
    );
    for field in [
        "utc_year",
        "utc_month",
        "utc_day",
        "utc_hour",
        "utc_minute",
        "utc_second",
    ] {
        exports
            .functions
            .insert(field.to_string(), pub_fn(vec![utc.clone()], Type::Int));
    }

    exports
}
