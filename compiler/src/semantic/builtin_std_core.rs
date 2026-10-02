use super::*;
use crate::ast::{FloatWidth, IntWidth, Type};
use crate::semantic::module_registry::{ExportVisibility, ExportedType, ModuleExports};

pub(crate) fn make_std_io() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "io".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    // print(value: any) -> unit
    // The runtime FFI accepts a single value and prints it.
    exports.functions.insert(
        "print".to_string(),
        pub_fn(
            vec![Type::TypeParameter {
                name: "T".to_string(),
            }],
            Type::Unit,
        ),
    );
    // println(value: any) -> unit  (print + newline)
    exports.functions.insert(
        "println".to_string(),
        pub_fn(
            vec![Type::TypeParameter {
                name: "T".to_string(),
            }],
            Type::Unit,
        ),
    );
    // eprint(value: any) -> unit  (stderr, no newline)
    exports.functions.insert(
        "eprint".to_string(),
        pub_fn(
            vec![Type::TypeParameter {
                name: "T".to_string(),
            }],
            Type::Unit,
        ),
    );
    // eprintln(value: any) -> unit
    exports.functions.insert(
        "eprintln".to_string(),
        pub_fn(
            vec![Type::TypeParameter {
                name: "T".to_string(),
            }],
            Type::Unit,
        ),
    );
    // flush() -> unit
    exports
        .functions
        .insert("flush".to_string(), pub_fn(vec![], Type::Unit));
    // read_line() -> string
    exports
        .functions
        .insert("read_line".to_string(), pub_fn(vec![], Type::String));
    // input(prompt: string) -> string  (prints prompt, flushes, reads line)
    exports.functions.insert(
        "input".to_string(),
        pub_fn(vec![Type::String], Type::String),
    );

    exports
}

pub(crate) fn make_std_math() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "math".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    exports
        .functions
        .insert("abs".to_string(), pub_fn(vec![Type::Int], Type::Int));
    exports.functions.insert(
        "min".to_string(),
        pub_fn(vec![Type::Int, Type::Int], Type::Int),
    );
    exports.functions.insert(
        "max".to_string(),
        pub_fn(vec![Type::Int, Type::Int], Type::Int),
    );
    exports.functions.insert(
        "clamp".to_string(),
        pub_fn(vec![Type::Int, Type::Int, Type::Int], Type::Int),
    );
    exports
        .functions
        .insert("sqrt_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports.functions.insert(
        "pow_f".to_string(),
        pub_fn(vec![Type::Float, Type::Float], Type::Float),
    );
    exports.functions.insert(
        "floor_f".to_string(),
        pub_fn(vec![Type::Float], Type::Float),
    );
    exports
        .functions
        .insert("ceil_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports.functions.insert(
        "round_f".to_string(),
        pub_fn(vec![Type::Float], Type::Float),
    );
    exports
        .functions
        .insert("sin_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports
        .functions
        .insert("cos_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports
        .functions
        .insert("tan_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports
        .functions
        .insert("log_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports
        .functions
        .insert("log2_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports.functions.insert(
        "log10_f".to_string(),
        pub_fn(vec![Type::Float], Type::Float),
    );
    exports.functions.insert(
        "atan2_f".to_string(),
        pub_fn(vec![Type::Float, Type::Float], Type::Float),
    );
    exports
        .functions
        .insert("pi".to_string(), pub_fn(vec![], Type::Float));
    exports
        .functions
        .insert("e_const".to_string(), pub_fn(vec![], Type::Float));
    // sign(n: int) -> int — returns -1, 0, or 1
    exports
        .functions
        .insert("sign".to_string(), pub_fn(vec![Type::Int], Type::Int));
    // gcd(a: int, b: int) -> int
    exports.functions.insert(
        "gcd".to_string(),
        pub_fn(vec![Type::Int, Type::Int], Type::Int),
    );
    // lcm(a: int, b: int) -> int
    exports.functions.insert(
        "lcm".to_string(),
        pub_fn(vec![Type::Int, Type::Int], Type::Int),
    );
    // is_nan_f(x: float) -> bool
    exports.functions.insert(
        "is_nan_f".to_string(),
        pub_fn(vec![Type::Float], Type::Bool),
    );
    // is_infinite_f(x: float) -> bool
    exports.functions.insert(
        "is_infinite_f".to_string(),
        pub_fn(vec![Type::Float], Type::Bool),
    );
    // abs_f(x: float) -> float
    exports
        .functions
        .insert("abs_f".to_string(), pub_fn(vec![Type::Float], Type::Float));

    exports
}

pub(crate) fn make_std_numeric() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "numeric".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };
    for (name, ty) in [
        (
            "i8",
            Type::ExactInt {
                signed: true,
                width: IntWidth::I8,
            },
        ),
        (
            "i16",
            Type::ExactInt {
                signed: true,
                width: IntWidth::I16,
            },
        ),
        (
            "i32",
            Type::ExactInt {
                signed: true,
                width: IntWidth::I32,
            },
        ),
        (
            "i64",
            Type::ExactInt {
                signed: true,
                width: IntWidth::I64,
            },
        ),
        (
            "u8",
            Type::ExactInt {
                signed: false,
                width: IntWidth::I8,
            },
        ),
        (
            "u16",
            Type::ExactInt {
                signed: false,
                width: IntWidth::I16,
            },
        ),
        (
            "u32",
            Type::ExactInt {
                signed: false,
                width: IntWidth::I32,
            },
        ),
        (
            "u64",
            Type::ExactInt {
                signed: false,
                width: IntWidth::I64,
            },
        ),
    ] {
        exports.functions.insert(
            format!("checked_{name}"),
            pub_fn(vec![Type::Int], ty.clone()),
        );
        exports.functions.insert(
            format!("checked_float_{name}"),
            pub_fn(vec![Type::Float], ty.clone()),
        );
        for op in ["add", "sub", "mul"] {
            exports.functions.insert(
                format!("checked_{op}_{name}"),
                pub_fn(vec![ty.clone(), ty.clone()], ty.clone()),
            );
        }
        for op in ["add", "sub", "mul"] {
            exports.functions.insert(
                format!("wrapping_{op}_{name}"),
                pub_fn(vec![ty.clone(), ty.clone()], ty.clone()),
            );
        }
    }
    exports.functions.insert(
        "checked_f32".to_string(),
        pub_fn(
            vec![Type::ExactFloat {
                width: FloatWidth::F64,
            }],
            Type::ExactFloat {
                width: FloatWidth::F32,
            },
        ),
    );
    exports
}

/// Public collection surface. Potentially empty reads are represented by
/// `Option<T>`.
pub(crate) fn make_std_collections() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "collections".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let list = Type::Struct {
        name: "List".to_string(),
    };
    let vector = Type::Struct {
        name: "Vector".to_string(),
    };
    let map = Type::Struct {
        name: "Map".to_string(),
    };
    let option = Type::Enum {
        name: "Option".to_string(),
    };
    let element = Type::TypeParameter {
        name: "T".to_string(),
    };
    let key = Type::TypeParameter {
        name: "K".to_string(),
    };
    let value = Type::TypeParameter {
        name: "V".to_string(),
    };
    let set = Type::Struct {
        name: "Set".to_string(),
    };
    let iterator = Type::Struct {
        name: "Iterator".to_string(),
    };
    let stack = Type::Struct {
        name: "Stack".to_string(),
    };
    let queue = Type::Struct {
        name: "Queue".to_string(),
    };
    let hash_set = Type::Struct {
        name: "HashSet".to_string(),
    };
    let ordered_map = Type::Struct {
        name: "OrderedMap".to_string(),
    };
    let priority_queue = Type::Struct {
        name: "PriorityQueue".to_string(),
    };
    let bit_set = Type::Struct {
        name: "BitSet".to_string(),
    };
    let disjoint_set = Type::Struct {
        name: "DisjointSet".to_string(),
    };

    exports
        .functions
        .insert("list_new".to_string(), pub_fn(vec![], list.clone()));
    exports
        .functions
        .insert("vector_new".to_string(), pub_fn(vec![], vector.clone()));
    exports.functions.insert(
        "vector_with_capacity".to_string(),
        pub_fn(vec![Type::Int], vector.clone()),
    );
    exports.functions.insert(
        "vector_capacity".to_string(),
        pub_fn(vec![vector.clone()], Type::Int),
    );
    exports.functions.insert(
        "vector_reserve".to_string(),
        pub_fn(vec![vector.clone(), Type::Int], Type::Unit),
    );
    exports.functions.insert(
        "vector_push".to_string(),
        pub_fn(vec![vector.clone(), element.clone()], Type::Unit),
    );
    exports.functions.insert(
        "vector_pop".to_string(),
        pub_fn(vec![vector.clone()], option.clone()),
    );
    exports.functions.insert(
        "vector_get".to_string(),
        pub_fn(vec![vector.clone(), Type::Int], option.clone()),
    );
    exports.functions.insert(
        "vector_set".to_string(),
        pub_fn(
            vec![vector.clone(), Type::Int, element.clone()],
            Type::Unit,
        ),
    );
    exports.functions.insert(
        "vector_insert_at".to_string(),
        pub_fn(
            vec![vector.clone(), Type::Int, element.clone()],
            Type::Unit,
        ),
    );
    exports.functions.insert(
        "vector_remove_at".to_string(),
        pub_fn(vec![vector.clone(), Type::Int], option.clone()),
    );
    exports.functions.insert(
        "vector_contains".to_string(),
        pub_fn(vec![vector.clone(), element.clone()], Type::Bool),
    );
    exports.functions.insert(
        "vector_index_of".to_string(),
        pub_fn(vec![vector.clone(), element.clone()], Type::Int),
    );
    exports.functions.insert(
        "vector_len".to_string(),
        pub_fn(vec![vector.clone()], Type::Int),
    );
    exports.functions.insert(
        "vector_is_empty".to_string(),
        pub_fn(vec![vector.clone()], Type::Bool),
    );
    exports.functions.insert(
        "vector_clear".to_string(),
        pub_fn(vec![vector.clone()], Type::Unit),
    );
    exports.functions.insert(
        "vector_iter".to_string(),
        pub_fn(vec![vector.clone()], iterator.clone()),
    );
    exports.functions.insert(
        "vector_free".to_string(),
        pub_fn(vec![vector.clone()], Type::Unit),
    );
    exports.functions.insert(
        "list_with_capacity".to_string(),
        pub_fn(vec![Type::Int], list.clone()),
    );
    exports.functions.insert(
        "list_reserve".to_string(),
        pub_fn(vec![list.clone(), Type::Int], Type::Unit),
    );
    exports.functions.insert(
        "list_capacity".to_string(),
        pub_fn(vec![list.clone()], Type::Int),
    );
    exports.functions.insert(
        "list_push".to_string(),
        pub_fn(vec![list.clone(), element.clone()], Type::Unit),
    );
    exports.functions.insert(
        "list_len".to_string(),
        pub_fn(vec![list.clone()], Type::Int),
    );
    exports.functions.insert(
        "list_get".to_string(),
        pub_fn(vec![list.clone(), Type::Int], option.clone()),
    );
    exports.functions.insert(
        "list_get_option".to_string(),
        pub_fn(vec![list.clone(), Type::Int], option.clone()),
    );
    exports.functions.insert(
        "list_set".to_string(),
        pub_fn(vec![list.clone(), Type::Int, element.clone()], Type::Unit),
    );
    exports.functions.insert(
        "list_contains".to_string(),
        pub_fn(vec![list.clone(), element.clone()], Type::Bool),
    );
    exports.functions.insert(
        "list_clear".to_string(),
        pub_fn(vec![list.clone()], Type::Unit),
    );
    exports.functions.insert(
        "list_free".to_string(),
        pub_fn(vec![list.clone()], Type::Unit),
    );
    // list_free_all() -> int
    exports
        .functions
        .insert("list_free_all".to_string(), pub_fn(vec![], Type::Int));
    exports.functions.insert(
        "list_pop".to_string(),
        pub_fn(vec![list.clone()], option.clone()),
    );
    exports.functions.insert(
        "list_pop_front".to_string(),
        pub_fn(vec![list.clone()], option.clone()),
    );
    exports.functions.insert(
        "list_pop_option".to_string(),
        pub_fn(vec![list.clone()], option.clone()),
    );
    exports.functions.insert(
        "list_pop_front_option".to_string(),
        pub_fn(vec![list.clone()], option.clone()),
    );
    exports.functions.insert(
        "list_insert_at".to_string(),
        pub_fn(vec![list.clone(), Type::Int, element.clone()], Type::Unit),
    );
    exports.functions.insert(
        "list_remove_at".to_string(),
        pub_fn(vec![list.clone(), Type::Int], option.clone()),
    );
    exports.functions.insert(
        "list_remove_at_option".to_string(),
        pub_fn(vec![list.clone(), Type::Int], option.clone()),
    );
    exports.functions.insert(
        "list_index_of".to_string(),
        pub_fn(vec![list.clone(), element.clone()], Type::Int),
    );
    exports.functions.insert(
        "list_sort".to_string(),
        pub_fn(vec![list.clone()], Type::Unit),
    );
    let fn_int_to_int = Type::Fn {
        params: vec![Type::Int],
        return_type: Box::new(Type::Int),
    };
    let fn_int_to_bool = Type::Fn {
        params: vec![Type::Int],
        return_type: Box::new(Type::Bool),
    };
    let fn_int_int_to_int = Type::Fn {
        params: vec![Type::Int, Type::Int],
        return_type: Box::new(Type::Int),
    };
    exports.functions.insert(
        "list_map".to_string(),
        pub_fn(vec![list.clone(), fn_int_to_int.clone()], list.clone()),
    );
    exports.functions.insert(
        "list_filter".to_string(),
        pub_fn(vec![list.clone(), fn_int_to_bool], list.clone()),
    );
    exports.functions.insert(
        "list_reduce".to_string(),
        pub_fn(
            vec![list.clone(), Type::Int, fn_int_int_to_int.clone()],
            Type::Int,
        ),
    );
    exports.functions.insert(
        "list_sort_by".to_string(),
        pub_fn(vec![list.clone(), fn_int_int_to_int], Type::Unit),
    );

    // â”€â”€ map API (R-3123: expose existing runtime HashMap<i64, i64>) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    exports
        .functions
        .insert("map_new".to_string(), pub_fn(vec![], map.clone()));
    exports.functions.insert(
        "map_with_capacity".to_string(),
        pub_fn(vec![Type::Int], map.clone()),
    );
    exports.functions.insert(
        "map_reserve".to_string(),
        pub_fn(vec![map.clone(), Type::Int], Type::Unit),
    );
    exports.functions.insert(
        "map_capacity".to_string(),
        pub_fn(vec![map.clone()], Type::Int),
    );
    exports.functions.insert(
        "map_set".to_string(),
        pub_fn(vec![map.clone(), key.clone(), value.clone()], Type::Unit),
    );
    exports.functions.insert(
        "map_get".to_string(),
        pub_fn(vec![map.clone(), key.clone()], option.clone()),
    );
    exports.functions.insert(
        "map_get_option".to_string(),
        pub_fn(vec![map.clone(), key.clone()], option.clone()),
    );
    exports.functions.insert(
        "map_contains".to_string(),
        pub_fn(vec![map.clone(), key.clone()], Type::Bool),
    );
    exports.functions.insert(
        "map_remove".to_string(),
        pub_fn(vec![map.clone(), key.clone()], option.clone()),
    );
    exports.functions.insert(
        "map_remove_option".to_string(),
        pub_fn(vec![map.clone(), key.clone()], option.clone()),
    );
    exports
        .functions
        .insert("map_len".to_string(), pub_fn(vec![map.clone()], Type::Int));
    exports.functions.insert(
        "map_is_empty".to_string(),
        pub_fn(vec![map.clone()], Type::Bool),
    );
    exports.functions.insert(
        "map_clear".to_string(),
        pub_fn(vec![map.clone()], Type::Unit),
    );
    exports
        .functions
        .insert("map_free_all".to_string(), pub_fn(vec![], Type::Int));
    exports
        .functions
        .insert("map_free".to_string(), pub_fn(vec![map], Type::Unit));

    // â”€â”€ stack and queue APIs â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    exports
        .functions
        .insert("stack_new".to_string(), pub_fn(vec![], stack.clone()));
    exports.functions.insert(
        "stack_with_capacity".to_string(),
        pub_fn(vec![Type::Int], stack.clone()),
    );
    exports.functions.insert(
        "stack_reserve".to_string(),
        pub_fn(vec![stack.clone(), Type::Int], Type::Unit),
    );
    exports.functions.insert(
        "stack_capacity".to_string(),
        pub_fn(vec![stack.clone()], Type::Int),
    );
    exports.functions.insert(
        "stack_push".to_string(),
        pub_fn(vec![stack.clone(), element.clone()], Type::Unit),
    );
    exports.functions.insert(
        "stack_pop".to_string(),
        pub_fn(vec![stack.clone()], option.clone()),
    );
    exports.functions.insert(
        "stack_peek".to_string(),
        pub_fn(vec![stack.clone()], option.clone()),
    );
    exports.functions.insert(
        "stack_len".to_string(),
        pub_fn(vec![stack.clone()], Type::Int),
    );
    exports.functions.insert(
        "stack_is_empty".to_string(),
        pub_fn(vec![stack.clone()], Type::Bool),
    );
    exports.functions.insert(
        "stack_clear".to_string(),
        pub_fn(vec![stack.clone()], Type::Unit),
    );
    exports.functions.insert(
        "stack_free".to_string(),
        pub_fn(vec![stack.clone()], Type::Unit),
    );
    exports
        .functions
        .insert("stack_free_all".to_string(), pub_fn(vec![], Type::Int));
    exports.functions.insert(
        "stack_iter".to_string(),
        pub_fn(vec![stack.clone()], iterator.clone()),
    );

    exports
        .functions
        .insert("queue_new".to_string(), pub_fn(vec![], queue.clone()));
    exports.functions.insert(
        "queue_with_capacity".to_string(),
        pub_fn(vec![Type::Int], queue.clone()),
    );
    exports.functions.insert(
        "queue_reserve".to_string(),
        pub_fn(vec![queue.clone(), Type::Int], Type::Unit),
    );
    exports.functions.insert(
        "queue_capacity".to_string(),
        pub_fn(vec![queue.clone()], Type::Int),
    );
    exports.functions.insert(
        "queue_enqueue".to_string(),
        pub_fn(vec![queue.clone(), element.clone()], Type::Unit),
    );
    exports.functions.insert(
        "queue_dequeue".to_string(),
        pub_fn(vec![queue.clone()], option.clone()),
    );
    exports.functions.insert(
        "queue_peek".to_string(),
        pub_fn(vec![queue.clone()], option.clone()),
    );
    exports.functions.insert(
        "queue_len".to_string(),
        pub_fn(vec![queue.clone()], Type::Int),
    );
    exports.functions.insert(
        "queue_is_empty".to_string(),
        pub_fn(vec![queue.clone()], Type::Bool),
    );
    exports.functions.insert(
        "queue_clear".to_string(),
        pub_fn(vec![queue.clone()], Type::Unit),
    );
    exports.functions.insert(
        "queue_free".to_string(),
        pub_fn(vec![queue.clone()], Type::Unit),
    );
    exports
        .functions
        .insert("queue_free_all".to_string(), pub_fn(vec![], Type::Int));
    exports.functions.insert(
        "queue_iter".to_string(),
        pub_fn(vec![queue], iterator.clone()),
    );

    // â”€â”€ set API â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    exports
        .functions
        .insert("set_new".to_string(), pub_fn(vec![], set.clone()));
    exports.functions.insert(
        "set_with_capacity".to_string(),
        pub_fn(vec![Type::Int], set.clone()),
    );
    exports.functions.insert(
        "set_reserve".to_string(),
        pub_fn(vec![set.clone(), Type::Int], Type::Unit),
    );
    exports.functions.insert(
        "set_capacity".to_string(),
        pub_fn(vec![set.clone()], Type::Int),
    );
    exports.functions.insert(
        "set_insert".to_string(),
        pub_fn(vec![set.clone(), element.clone()], Type::Bool),
    );
    exports.functions.insert(
        "set_contains".to_string(),
        pub_fn(vec![set.clone(), element.clone()], Type::Bool),
    );
    exports.functions.insert(
        "set_remove".to_string(),
        pub_fn(vec![set.clone(), element.clone()], Type::Bool),
    );
    exports
        .functions
        .insert("set_len".to_string(), pub_fn(vec![set.clone()], Type::Int));
    exports.functions.insert(
        "set_get".to_string(),
        pub_fn(
            vec![set.clone(), Type::Int],
            Type::Enum {
                name: "Option".to_string(),
            },
        ),
    );
    exports.functions.insert(
        "set_clear".to_string(),
        pub_fn(vec![set.clone()], Type::Unit),
    );
    exports
        .functions
        .insert("set_free".to_string(), pub_fn(vec![set], Type::Unit));

    // â”€â”€ Iterator protocol â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    exports.functions.insert(
        "list_iter".to_string(),
        pub_fn(vec![list.clone()], iterator.clone()),
    );
    exports.functions.insert(
        "set_iter".to_string(),
        pub_fn(
            vec![Type::Struct {
                name: "Set".to_string(),
            }],
            iterator.clone(),
        ),
    );
    exports.functions.insert(
        "map_iter".to_string(),
        pub_fn(
            vec![Type::Struct {
                name: "Map".to_string(),
            }],
            iterator.clone(),
        ),
    );
    exports.functions.insert(
        "map_values_iter".to_string(),
        pub_fn(
            vec![Type::Struct {
                name: "Map".to_string(),
            }],
            iterator.clone(),
        ),
    );
    exports.functions.insert(
        "iterator_next".to_string(),
        pub_fn(
            vec![iterator.clone()],
            Type::Enum {
                name: "Option".to_string(),
            },
        ),
    );
    exports.functions.insert(
        "iterator_remaining".to_string(),
        pub_fn(vec![iterator.clone()], Type::Int),
    );
    exports.functions.insert(
        "iterator_free".to_string(),
        pub_fn(vec![iterator.clone()], Type::Unit),
    );

    // â”€â”€ additional collection APIs (Phase 33) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let add_collection_fn =
        |exports: &mut ModuleExports, name: &str, params: Vec<Type>, return_type: Type| {
            exports
                .functions
                .insert(name.to_string(), pub_fn(params, return_type));
        };
    for (name, params, return_type) in [
        ("hash_set_new", vec![], hash_set.clone()),
        ("hash_set_with_capacity", vec![Type::Int], hash_set.clone()),
        ("hash_set_capacity", vec![hash_set.clone()], Type::Int),
        (
            "hash_set_insert",
            vec![hash_set.clone(), element.clone()],
            Type::Bool,
        ),
        (
            "hash_set_contains",
            vec![hash_set.clone(), element.clone()],
            Type::Bool,
        ),
        (
            "hash_set_remove",
            vec![hash_set.clone(), element.clone()],
            Type::Bool,
        ),
        ("hash_set_len", vec![hash_set.clone()], Type::Int),
        ("hash_set_clear", vec![hash_set.clone()], Type::Unit),
        ("hash_set_iter", vec![hash_set.clone()], iterator.clone()),
        ("hash_set_free", vec![hash_set.clone()], Type::Unit),
        ("ordered_map_new", vec![], ordered_map.clone()),
        (
            "ordered_map_set",
            vec![ordered_map.clone(), key.clone(), value.clone()],
            Type::Unit,
        ),
        (
            "ordered_map_get",
            vec![ordered_map.clone(), key.clone()],
            option.clone(),
        ),
        (
            "ordered_map_contains",
            vec![ordered_map.clone(), key.clone()],
            Type::Bool,
        ),
        (
            "ordered_map_remove",
            vec![ordered_map.clone(), key.clone()],
            option.clone(),
        ),
        ("ordered_map_len", vec![ordered_map.clone()], Type::Int),
        (
            "ordered_map_iter",
            vec![ordered_map.clone()],
            iterator.clone(),
        ),
        (
            "ordered_map_range_keys",
            vec![ordered_map.clone(), key.clone(), key.clone()],
            iterator.clone(),
        ),
        ("ordered_map_free", vec![ordered_map.clone()], Type::Unit),
        ("priority_queue_new", vec![], priority_queue.clone()),
        ("priority_queue_new_min", vec![], priority_queue.clone()),
        (
            "priority_queue_with_capacity",
            vec![Type::Int],
            priority_queue.clone(),
        ),
        (
            "priority_queue_capacity",
            vec![priority_queue.clone()],
            Type::Int,
        ),
        (
            "priority_queue_push",
            vec![priority_queue.clone(), element.clone()],
            Type::Unit,
        ),
        (
            "priority_queue_peek",
            vec![priority_queue.clone()],
            option.clone(),
        ),
        (
            "priority_queue_pop",
            vec![priority_queue.clone()],
            option.clone(),
        ),
        (
            "priority_queue_len",
            vec![priority_queue.clone()],
            Type::Int,
        ),
        (
            "priority_queue_clear",
            vec![priority_queue.clone()],
            Type::Unit,
        ),
        (
            "priority_queue_free",
            vec![priority_queue.clone()],
            Type::Unit,
        ),
        ("bitset_new", vec![], bit_set.clone()),
        ("bitset_with_capacity", vec![Type::Int], bit_set.clone()),
        ("bitset_capacity", vec![bit_set.clone()], Type::Int),
        (
            "bitset_insert",
            vec![bit_set.clone(), Type::Int],
            Type::Bool,
        ),
        (
            "bitset_remove",
            vec![bit_set.clone(), Type::Int],
            Type::Bool,
        ),
        (
            "bitset_contains",
            vec![bit_set.clone(), Type::Int],
            Type::Bool,
        ),
        ("bitset_count", vec![bit_set.clone()], Type::Int),
        (
            "bitset_union_with",
            vec![bit_set.clone(), bit_set.clone()],
            Type::Unit,
        ),
        (
            "bitset_intersect_with",
            vec![bit_set.clone(), bit_set.clone()],
            Type::Unit,
        ),
        (
            "bitset_difference_with",
            vec![bit_set.clone(), bit_set.clone()],
            Type::Unit,
        ),
        ("bitset_iter", vec![bit_set.clone()], iterator.clone()),
        ("bitset_free", vec![bit_set.clone()], Type::Unit),
        ("disjoint_set_new", vec![Type::Int], disjoint_set.clone()),
        ("disjoint_set_add", vec![disjoint_set.clone()], Type::Int),
        (
            "disjoint_set_find",
            vec![disjoint_set.clone(), Type::Int],
            Type::Int,
        ),
        (
            "disjoint_set_union",
            vec![disjoint_set.clone(), Type::Int, Type::Int],
            Type::Bool,
        ),
        (
            "disjoint_set_connected",
            vec![disjoint_set.clone(), Type::Int, Type::Int],
            Type::Bool,
        ),
        ("disjoint_set_count", vec![disjoint_set.clone()], Type::Int),
        ("disjoint_set_free", vec![disjoint_set.clone()], Type::Unit),
    ] {
        add_collection_fn(&mut exports, name, params, return_type);
    }

    // type aliases
    exports.types.insert(
        "List".to_string(),
        ExportedType {
            members: vec![
                "new".to_string(),
                "with_capacity".to_string(),
                "reserve".to_string(),
                "capacity".to_string(),
                "push".to_string(),
                "len".to_string(),
            ],
            visibility: ExportVisibility::Public,
            is_enum: false,
            type_params: Vec::new(),
            struct_field_visibility: None,
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );
    exports.types.insert(
        "Map".to_string(),
        ExportedType {
            members: vec![
                "new".to_string(),
                "with_capacity".to_string(),
                "reserve".to_string(),
                "capacity".to_string(),
                "set".to_string(),
                "get".to_string(),
            ],
            visibility: ExportVisibility::Public,
            is_enum: false,
            type_params: Vec::new(),
            struct_field_visibility: None,
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );
    for (name, members) in [
        (
            "Vector",
            vec![
                "new",
                "with_capacity",
                "reserve",
                "capacity",
                "push",
                "pop",
                "get",
                "set",
                "insert_at",
                "remove_at",
                "contains",
                "index_of",
                "len",
                "is_empty",
                "clear",
                "iter",
            ],
        ),
        (
            "Stack",
            vec![
                "new",
                "with_capacity",
                "reserve",
                "capacity",
                "push",
                "pop",
                "len",
            ],
        ),
        (
            "Queue",
            vec![
                "new",
                "with_capacity",
                "reserve",
                "capacity",
                "enqueue",
                "dequeue",
                "len",
            ],
        ),
    ] {
        exports.types.insert(
            name.to_string(),
            ExportedType {
                members: members
                    .into_iter()
                    .map(|member| member.to_string())
                    .collect(),
                visibility: ExportVisibility::Public,
                is_enum: false,
                type_params: Vec::new(),
                struct_field_visibility: None,
                struct_fields: None,
                enum_variants: None,
                enum_struct_variants: None,
            },
        );
    }
    for (name, members) in [
        (
            "HashSet",
            vec![
                "new",
                "with_capacity",
                "insert",
                "contains",
                "remove",
                "len",
                "clear",
                "iter",
                "capacity",
            ],
        ),
        (
            "OrderedMap",
            vec![
                "new",
                "set",
                "get",
                "contains",
                "remove",
                "len",
                "iter",
                "range_keys",
            ],
        ),
        (
            "PriorityQueue",
            vec![
                "new",
                "new_min",
                "with_capacity",
                "capacity",
                "push",
                "peek",
                "pop",
                "len",
                "clear",
            ],
        ),
        (
            "BitSet",
            vec![
                "new",
                "with_capacity",
                "insert",
                "remove",
                "contains",
                "count",
                "union_with",
                "intersect_with",
                "difference_with",
                "iter",
                "capacity",
            ],
        ),
        (
            "DisjointSet",
            vec!["new", "add", "find", "union", "connected", "count"],
        ),
    ] {
        exports.types.insert(
            name.to_string(),
            ExportedType {
                members: members.into_iter().map(str::to_string).collect(),
                visibility: ExportVisibility::Public,
                is_enum: false,
                type_params: Vec::new(),
                struct_field_visibility: None,
                struct_fields: None,
                enum_variants: None,
                enum_struct_variants: None,
            },
        );
    }
    for name in ["Set", "Iterator"] {
        exports.types.insert(
            name.to_string(),
            ExportedType {
                members: if name == "Set" {
                    vec![
                        "new".to_string(),
                        "with_capacity".to_string(),
                        "reserve".to_string(),
                        "capacity".to_string(),
                        "len".to_string(),
                    ]
                } else {
                    vec!["new".to_string(), "len".to_string()]
                },
                visibility: ExportVisibility::Public,
                is_enum: false,
                type_params: Vec::new(),
                struct_field_visibility: None,
                struct_fields: None,
                enum_variants: None,
                enum_struct_variants: None,
            },
        );
    }

    exports
}

pub(crate) fn make_std_tensor() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "tensor".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let int = Type::Int;
    let float = Type::Float;
    let tensor_float_rank1 = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: Some(1),
        dims: None,
        layout: None,
        device: None,
    };
    let tensor_float_rank2 = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: Some(2),
        dims: None,
        layout: None,
        device: None,
    };
    let tensor_float_rank0 = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: Some(0),
        dims: None,
        layout: None,
        device: None,
    };
    let tensor_float_dynamic = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: None,
        dims: None,
        layout: None,
        device: None,
    };
    let unit = Type::Unit;
    let bool_ty = Type::Bool;

    let functions = [
        (
            "vector_f",
            vec![int.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        (
            "matrix_f",
            vec![int.clone(), int.clone(), float.clone()],
            tensor_float_rank2.clone(),
        ),
        ("zeros", vec![int.clone()], int.clone()),
        ("ones", vec![int.clone()], int.clone()),
        ("full", vec![int.clone(), int.clone()], int.clone()),
        (
            "full_f",
            vec![int.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        (
            "arange",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("zeros2", vec![int.clone(), int.clone()], int.clone()),
        ("ones2", vec![int.clone(), int.clone()], int.clone()),
        (
            "full2",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "full2_f",
            vec![int.clone(), int.clone(), float.clone()],
            tensor_float_rank2.clone(),
        ),
        ("len", vec![int.clone()], int.clone()),
        ("rank", vec![int.clone()], int.clone()),
        ("dim", vec![int.clone(), int.clone()], int.clone()),
        ("rows", vec![int.clone()], int.clone()),
        ("cols", vec![int.clone()], int.clone()),
        ("is_valid", vec![int.clone()], bool_ty.clone()),
        ("get", vec![int.clone(), int.clone()], int.clone()),
        ("get_f", vec![int.clone(), int.clone()], float.clone()),
        (
            "set",
            vec![int.clone(), int.clone(), int.clone()],
            unit.clone(),
        ),
        (
            "set_f",
            vec![int.clone(), int.clone(), float.clone()],
            unit.clone(),
        ),
        (
            "get2",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "get2_f",
            vec![int.clone(), int.clone(), int.clone()],
            float.clone(),
        ),
        (
            "set2",
            vec![int.clone(), int.clone(), int.clone(), int.clone()],
            unit.clone(),
        ),
        (
            "set2_f",
            vec![int.clone(), int.clone(), int.clone(), float.clone()],
            unit.clone(),
        ),
        (
            "reshape",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("flatten", vec![int.clone()], int.clone()),
        (
            "permute",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "slice",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("concat", vec![int.clone(), int.clone()], int.clone()),
        ("stack", vec![int.clone(), int.clone()], int.clone()),
        (
            "add",
            vec![int.clone(), int.clone()],
            tensor_float_dynamic.clone(),
        ),
        (
            "sub",
            vec![int.clone(), int.clone()],
            tensor_float_dynamic.clone(),
        ),
        (
            "mul",
            vec![int.clone(), int.clone()],
            tensor_float_dynamic.clone(),
        ),
        (
            "div",
            vec![int.clone(), int.clone()],
            tensor_float_dynamic.clone(),
        ),
        ("sum", vec![int.clone()], int.clone()),
        ("sum_f", vec![int.clone()], float.clone()),
        ("sum_t", vec![int.clone()], tensor_float_rank0.clone()),
        ("mean_f", vec![int.clone()], float.clone()),
        ("mean_t", vec![int.clone()], tensor_float_rank0.clone()),
        ("max", vec![int.clone()], int.clone()),
        ("min", vec![int.clone()], int.clone()),
        ("argmax", vec![int.clone()], int.clone()),
        (
            "matmul",
            vec![int.clone(), int.clone()],
            tensor_float_rank2.clone(),
        ),
        (
            "matmul_batched",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        ("transpose", vec![int.clone()], tensor_float_rank2.clone()),
        ("dot", vec![int.clone(), int.clone()], int.clone()),
        (
            "dot_t",
            vec![int.clone(), int.clone()],
            tensor_float_rank0.clone(),
        ),
        ("neg", vec![int.clone()], tensor_float_dynamic.clone()),
        ("exp_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("log_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("sqrt_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("relu", vec![int.clone()], tensor_float_dynamic.clone()),
        ("sigmoid_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("tanh_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("seed", vec![int.clone()], unit.clone()),
        (
            "uniform",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "uniform_f",
            vec![int.clone(), float.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        (
            "normal_f",
            vec![int.clone(), float.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        (
            "bernoulli",
            vec![int.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        ("categorical", vec![int.clone(), int.clone()], int.clone()),
        ("set_deterministic_mode", vec![int.clone()], int.clone()),
        ("deterministic_mode", vec![], int.clone()),
        ("tolerance_abs", vec![], float.clone()),
        ("tolerance_rel", vec![], float.clone()),
        ("device", vec![int.clone()], int.clone()),
        ("device_available", vec![int.clone()], bool_ty.clone()),
        ("device_status", vec![int.clone()], int.clone()),
        ("to_device", vec![int.clone(), int.clone()], int.clone()),
        ("cpu", vec![int.clone()], int.clone()),
        ("sync", vec![int.clone()], unit.clone()),
        ("precision", vec![int.clone()], int.clone()),
        ("to_precision", vec![int.clone(), int.clone()], int.clone()),
        ("stats_allocations", vec![], int.clone()),
        ("stats_active", vec![], int.clone()),
        ("stats_peak_bytes", vec![], int.clone()),
        ("stats_reused_buffers", vec![], int.clone()),
        ("stats_pool_hits", vec![], int.clone()),
        ("stats_pool_misses", vec![], int.clone()),
        ("stats_active_bytes", vec![], int.clone()),
        ("stats_scratch_reuses", vec![], int.clone()),
        ("kernel_strategy", vec![], int.clone()),
        ("stats_kernel_ops", vec![], int.clone()),
        ("stats_kernel_elements", vec![], int.clone()),
        ("stats_device_transfers", vec![], int.clone()),
        ("stats_gpu_kernel_ops", vec![], int.clone()),
        ("stats_cpu_fallbacks", vec![], int.clone()),
        ("stats_gpu_errors", vec![int.clone()], int.clone()),
        ("stats_device_pool_hits", vec![], int.clone()),
        ("stats_device_pool_misses", vec![], int.clone()),
        ("stats_device_pool_bytes_resident", vec![], int.clone()),
        ("storage_device", vec![int.clone()], int.clone()),
        ("stats_device_resident_tensors", vec![], int.clone()),
        ("stats_gpu_backward_ops", vec![], int.clone()),
        ("stats_graph_nodes", vec![], int.clone()),
        ("stats_lifetime_records", vec![], int.clone()),
        ("stats_released_lifetimes", vec![], int.clone()),
        ("stats_allocation_sites", vec![], int.clone()),
        ("stats_reuse_rate_per_mille", vec![], int.clone()),
        ("memory_report", vec![], Type::String),
        ("reset_stats", vec![], unit.clone()),
        (
            "requires_grad",
            vec![int.clone(), bool_ty.clone()],
            tensor_float_dynamic.clone(),
        ),
        ("diff", vec![tensor_float_rank0.clone()], unit.clone()),
        ("backward", vec![tensor_float_rank0.clone()], unit.clone()),
        ("grad", vec![int.clone()], tensor_float_dynamic.clone()),
        ("zero_grad", vec![int.clone()], unit.clone()),
        ("set_grad_enabled", vec![bool_ty.clone()], unit.clone()),
        ("grad_enabled", vec![], bool_ty.clone()),
        ("free", vec![int.clone()], unit.clone()),
        ("free_all", vec![], int.clone()),
        ("refill", vec![int.clone(), float.clone()], unit.clone()),
    ];

    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }

    exports.types.insert(
        "Tensor".to_string(),
        ExportedType {
            members: vec![
                "shape".to_string(),
                "dtype".to_string(),
                "device".to_string(),
                "precision".to_string(),
                "layout".to_string(),
            ],
            visibility: ExportVisibility::Public,
            is_enum: false,
            type_params: Vec::new(),
            struct_field_visibility: None,
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );

    exports
}
