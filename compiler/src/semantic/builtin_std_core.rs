use super::*;
use crate::ast::{FloatWidth, IntWidth, Type, TypeAnnotation, TypeAnnotationKind};
use crate::semantic::module_registry::{
    ExportVisibility, ExportedFunction, ExportedType, ModuleExports,
};
use crate::span::Span;

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

/// An async `std.agent` export: the compiler-visible signature is the
/// *output* type; `is_async` plus the explicit `Task<T>` return mirrors
/// [`crate::semantic::semantic_type_system::SemanticAnalyzer::async_task_type`]
/// so import sites observe exactly `Task<output>`.
fn agent_async_fn(params: Vec<Type>, output: Type) -> ExportedFunction {
    ExportedFunction {
        params,
        return_type: Type::Task {
            output: Box::new(output),
        },
        visibility: ExportVisibility::Public,
        is_async: true,
    }
}

/// A `std.agent` record declared purely for typed authoring and `surface`
/// documentation. The host ABI has no record channel, so `AgentSpec`/`Report`
/// travel as JSON documents and are decoded in user code with the existing
/// JSON derive (plan adaptation 11).
fn agent_record(members: &[(&str, TypeAnnotation)]) -> ExportedType {
    ExportedType {
        members: members.iter().map(|(name, _)| (*name).to_string()).collect(),
        visibility: ExportVisibility::Public,
        is_enum: false,
        struct_fields: Some(
            members
                .iter()
                .map(|(name, ty)| ((*name).to_string(), ty.clone()))
                .collect(),
        ),
        enum_variants: None,
        enum_struct_variants: None,
    }
}

/// `std.agent` — Phase 32 agent-platform namespace.
///
/// R-3209 landed the namespace seam (`token_count`); R-3211 adds the model
/// gateway and run lifecycle; R-3212 adds `remember`/`recall`.
/// `Run`/`ChunkStream` are opaque handle types;
/// `AgentSpec`/`Report` are the authored record shapes carried as JSON across
/// the host ABI (no record-value channel exists).
pub(crate) fn make_std_agent() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "agent".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    // ── types ────────────────────────────────────────────────────────────
    // Opaque generational handles owned by the runtime run/stream tables.
    exports.types.insert("Run".to_string(), public_type(&[]));
    exports.types.insert("ChunkStream".to_string(), public_type(&[]));
    exports.types.insert(
        "AgentSpec".to_string(),
        agent_record(&[
            ("goal", builtin_type_annotation("string")),
            ("model", builtin_type_annotation("string")),
            ("endpoint", builtin_type_annotation("string")),
            (
                "allow",
                TypeAnnotation {
                    kind: TypeAnnotationKind::Generic {
                        name: "List".to_string(),
                        type_args: vec![builtin_type_annotation("string")],
                    },
                    span: Span::dummy(),
                },
            ),
            ("max_tokens", builtin_type_annotation("int")),
            ("max_cost_micros", builtin_type_annotation("int")),
            ("max_seconds", builtin_type_annotation("int")),
            ("max_tool_calls", builtin_type_annotation("int")),
            ("untrusted", builtin_type_annotation("string")),
            ("seed", builtin_type_annotation("int")),
            ("journal", builtin_type_annotation("string")),
            ("journal_payloads", builtin_type_annotation("bool")),
            ("run_id", builtin_type_annotation("string")),
        ]),
    );
    exports.types.insert(
        "Report".to_string(),
        agent_record(&[
            ("status", builtin_type_annotation("string")),
            ("steps", builtin_type_annotation("int")),
            ("tool_calls", builtin_type_annotation("int")),
            ("tokens_in", builtin_type_annotation("int")),
            ("tokens_out", builtin_type_annotation("int")),
            ("cost_micros", builtin_type_annotation("int")),
            ("elapsed_ms", builtin_type_annotation("int")),
            ("compensations_pending", builtin_type_annotation("int")),
        ]),
    );

    // ── functions ────────────────────────────────────────────────────────
    let std_error = Type::Struct {
        name: "Error".to_string(),
    };
    let run = Type::Struct {
        name: "Run".to_string(),
    };
    let chunk_stream = Type::Struct {
        name: "ChunkStream".to_string(),
    };
    let float_tensor = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: Some(1),
        dims: None,
        layout: None,
        device: None,
    };
    let result_of = |ok: Type| Type::Applied {
        name: "Result".to_string(),
        args: vec![ok, std_error.clone()],
    };

    // token_count(text: string) -> int
    exports.functions.insert(
        "token_count".to_string(),
        pub_fn(vec![Type::String], Type::Int),
    );
    // agent_start(spec_json: string) -> Result<Run, Error>
    exports.functions.insert(
        "agent_start".to_string(),
        pub_fn(vec![Type::String], result_of(run.clone())),
    );
    // agent_end(run: Run) -> Result<string, Error>
    exports.functions.insert(
        "agent_end".to_string(),
        pub_fn(vec![run.clone()], result_of(Type::String)),
    );
    // ask(run: Run, prompt: string) -> Result<string, Error> (async)
    exports.functions.insert(
        "ask".to_string(),
        agent_async_fn(vec![run.clone(), Type::String], result_of(Type::String)),
    );
    // ask_json(run: Run, prompt: string, schema: string) -> Result<string, Error> (async)
    exports.functions.insert(
        "ask_json".to_string(),
        agent_async_fn(
            vec![run.clone(), Type::String, Type::String],
            result_of(Type::String),
        ),
    );
    // ask_stream(run: Run, prompt: string) -> Result<ChunkStream, Error> (async)
    exports.functions.insert(
        "ask_stream".to_string(),
        agent_async_fn(vec![run.clone(), Type::String], result_of(chunk_stream.clone())),
    );
    // stream_next(stream: ChunkStream) -> Result<string, Error> (async; "" ends)
    exports.functions.insert(
        "stream_next".to_string(),
        agent_async_fn(vec![chunk_stream.clone()], result_of(Type::String)),
    );
    // stream_close(stream: ChunkStream) -> Result<bool, Error> (async; idempotent)
    exports.functions.insert(
        "stream_close".to_string(),
        agent_async_fn(vec![chunk_stream.clone()], result_of(Type::Bool)),
    );
    // embed(run: Run, text: string) -> Result<Tensor, Error> (async; 1-D float)
    exports.functions.insert(
        "embed".to_string(),
        agent_async_fn(vec![run.clone(), Type::String], result_of(float_tensor)),
    );
    // budget_remaining(run: Run) -> Result<int, Error> (sync; R-3216 T1)
    // The int is the tokens left before `max_tokens`; `i64::MAX` when the
    // spec declares no token ceiling.
    exports.functions.insert(
        "budget_remaining".to_string(),
        pub_fn(vec![run.clone()], result_of(Type::Int)),
    );
    // remember(run: Run, text: string) -> Result<bool, Error> (sync; R-3212)
    exports.functions.insert(
        "remember".to_string(),
        pub_fn(vec![run.clone(), Type::String], result_of(Type::Bool)),
    );
    // recall(run: Run, query: string, top_k: int) -> Result<string, Error> (sync; R-3212)
    exports.functions.insert(
        "recall".to_string(),
        pub_fn(
            vec![run.clone(), Type::String, Type::Int],
            result_of(Type::String),
        ),
    );
    // act(run: Run, prompt: string) -> Result<string, Error> (async; R-3222)
    // Runs model<->tool turns until the model answers without a tool call.
    exports.functions.insert(
        "act".to_string(),
        agent_async_fn(vec![run.clone(), Type::String], result_of(Type::String)),
    );
    // tool_call(run: Run, name: string, args_json: string) -> Result<string, Error>
    // (async; R-3222). The single dispatcher primitive `act` is built on: it
    // invokes one registered `#[agent_tool]` wrapper by address through the
    // governed dispatch and returns its JSON result.
    exports.functions.insert(
        "tool_call".to_string(),
        agent_async_fn(
            vec![run.clone(), Type::String, Type::String],
            result_of(Type::String),
        ),
    );
    // register_tool(name, wrapper_address, description, input_schema, effects)
    // -> Result<bool, Error> (sync; R-3222).
    //
    // Emitted only by the compiler: lowering registers each tool's synthesized
    // marshalling wrapper before the first dispatch of a module. The entry is
    // `internal` so user code cannot name an arbitrary code address as a tool
    // wrapper; it exists in the contract so the catalog records the binding.
    exports.functions.insert(
        "register_tool".to_string(),
        ExportedFunction {
            params: vec![
                Type::String,
                Type::Int,
                Type::String,
                Type::String,
                Type::String,
            ],
            return_type: result_of(Type::Bool),
            visibility: ExportVisibility::Internal,
            is_async: false,
        },
    );
    // approve(run: Run, action: string) -> Result<bool, Error> (sync; R-3217)
    // True when the action is authorized. No approver attached means deny.
    exports.functions.insert(
        "approve".to_string(),
        pub_fn(vec![run.clone(), Type::String], result_of(Type::Bool)),
    );
    // require(run: Run, condition: bool, message: string)
    // -> Result<bool, Error> (sync; R-3217). False returns a typed error
    // naming the message and the run goal and marks the run failed.
    exports.functions.insert(
        "require".to_string(),
        pub_fn(
            vec![run.clone(), Type::Bool, Type::String],
            result_of(Type::Bool),
        ),
    );
    // untrusted(run: Run, value: string, origin: string) -> Result<string, Error>
    // (sync; R-3223). Records that `value` entered the run from `origin` and
    // returns the value unchanged, so a caller can tag content inline.
    exports.functions.insert(
        "untrusted".to_string(),
        pub_fn(
            vec![run.clone(), Type::String, Type::String],
            result_of(Type::String),
        ),
    );
    // trust(run: Run, value: string, reason: string) -> Result<string, Error>
    // (sync; R-3223). Declassifies the digest of `value` and returns it
    // unchanged; the reason is mandatory and is the audit record.
    exports.functions.insert(
        "trust".to_string(),
        pub_fn(
            vec![run.clone(), Type::String, Type::String],
            result_of(Type::String),
        ),
    );
    // compensate(run: Run, tool: string, arguments_json: string)
    // -> Result<bool, Error> (sync; R-3224). Journals a pending compensation
    // (LIFO) after validating the tool name against the run's registry.
    // Literal tool names are additionally checked at compile time (E3205).
    exports.functions.insert(
        "compensate".to_string(),
        pub_fn(
            vec![run.clone(), Type::String, Type::String],
            result_of(Type::Bool),
        ),
    );
    // rollback(run: Run, reason: string) -> Result<int, Error> (sync; R-3224).
    // Executes the pending compensations in LIFO order through the governed
    // dispatch and returns the number executed.
    exports.functions.insert(
        "rollback".to_string(),
        pub_fn(vec![run.clone(), Type::String], result_of(Type::Int)),
    );

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

    exports
        .functions
        .insert("list_new".to_string(), pub_fn(vec![], list.clone()));
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

    // ── map API (R-3123: expose existing runtime HashMap<i64, i64>) ──────────
    exports
        .functions
        .insert("map_new".to_string(), pub_fn(vec![], map.clone()));
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
        pub_fn(vec![map.clone(), key], option),
    );
    exports
        .functions
        .insert("map_len".to_string(), pub_fn(vec![map.clone()], Type::Int));
    exports.functions.insert(
        "map_clear".to_string(),
        pub_fn(vec![map.clone()], Type::Unit),
    );
    exports
        .functions
        .insert("map_free".to_string(), pub_fn(vec![map], Type::Unit));

    // ── set API ───────────────────────────────────────────────────────────
    exports
        .functions
        .insert("set_new".to_string(), pub_fn(vec![], set.clone()));
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

    // ── Iterator protocol ─────────────────────────────────────────────────
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
        pub_fn(vec![iterator], Type::Unit),
    );

    // type aliases
    exports.types.insert(
        "List".to_string(),
        ExportedType {
            members: vec!["new".to_string(), "push".to_string(), "len".to_string()],
            visibility: ExportVisibility::Public,
            is_enum: false,
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );
    exports.types.insert(
        "Map".to_string(),
        ExportedType {
            members: vec!["new".to_string(), "set".to_string(), "get".to_string()],
            visibility: ExportVisibility::Public,
            is_enum: false,
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );
    for name in ["Set", "Iterator"] {
        exports.types.insert(
            name.to_string(),
            ExportedType {
                members: vec!["new".to_string(), "len".to_string()],
                visibility: ExportVisibility::Public,
                is_enum: false,
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
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );

    exports
}
