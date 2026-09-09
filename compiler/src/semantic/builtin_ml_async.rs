use super::*;
use crate::semantic::module_registry::{ExportVisibility, ExportedType, ModuleExports};
use crate::ast::{Type};

pub(crate) fn make_std_ml() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "ml".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let int = Type::Int;
    let float = Type::Float;
    let unit = Type::Unit;
    let bool_ty = Type::Bool;
    let tensor_float_rank0 = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: Some(0),
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

    let functions = [
        ("module_new", vec![], int.clone()),
        (
            "module_add_parameter",
            vec![int.clone(), int.clone()],
            unit.clone(),
        ),
        ("module_parameter_count", vec![int.clone()], int.clone()),
        (
            "module_parameter",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "module_set_training",
            vec![int.clone(), bool_ty.clone()],
            unit.clone(),
        ),
        ("module_is_training", vec![int.clone()], bool_ty.clone()),
        (
            "linear",
            vec![int.clone(), int.clone(), int.clone()],
            tensor_float_rank2.clone(),
        ),
        (
            "conv2d",
            vec![
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
            ],
            int.clone(),
        ),
        (
            "dropout",
            vec![int.clone(), float.clone(), bool_ty.clone()],
            int.clone(),
        ),
        (
            "max_pool2d",
            vec![
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
            ],
            int.clone(),
        ),
        (
            "mse_loss",
            vec![int.clone(), int.clone()],
            tensor_float_rank0.clone(),
        ),
        (
            "bce_loss",
            vec![int.clone(), int.clone()],
            tensor_float_rank0.clone(),
        ),
        (
            "cross_entropy_loss",
            vec![int.clone(), int.clone()],
            tensor_float_rank0.clone(),
        ),
        (
            "nll_loss",
            vec![int.clone(), int.clone()],
            tensor_float_rank0.clone(),
        ),
        ("sgd_step", vec![int.clone(), float.clone()], unit.clone()),
        (
            "sgd_momentum_step",
            vec![int.clone(), int.clone(), float.clone(), float.clone()],
            unit.clone(),
        ),
        (
            "adam_step",
            vec![
                int.clone(),
                int.clone(),
                int.clone(),
                float.clone(),
                float.clone(),
                float.clone(),
                float.clone(),
                int.clone(),
            ],
            unit.clone(),
        ),
        (
            "adamw_step",
            vec![
                int.clone(),
                int.clone(),
                int.clone(),
                float.clone(),
                float.clone(),
                float.clone(),
                float.clone(),
                int.clone(),
                float.clone(),
            ],
            unit.clone(),
        ),
        (
            "exp_lr",
            vec![float.clone(), float.clone(), int.clone()],
            float.clone(),
        ),
        (
            "unscale_grad",
            vec![int.clone(), float.clone()],
            unit.clone(),
        ),
        (
            "dataset_from_tensors",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "dataset_from_csv",
            vec![Type::String, int.clone(), int.clone()],
            int.clone(),
        ),
        ("dataset_from_jsonl", vec![Type::String], int.clone()),
        (
            "dataset_from_npy",
            vec![Type::String, Type::String, int.clone()],
            int.clone(),
        ),
        ("dataset_from_directory", vec![Type::String], int.clone()),
        ("dataset_len", vec![int.clone()], int.clone()),
        (
            "dataset_map_features",
            vec![int.clone(), float.clone(), float.clone()],
            int.clone(),
        ),
        (
            "dataset_filter_label_min",
            vec![int.clone(), float.clone()],
            int.clone(),
        ),
        (
            "dataset_train_split",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "dataset_test_split",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "dataloader_new",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("dataloader_batch_count", vec![int.clone()], int.clone()),
        (
            "dataloader_batch_features",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "dataloader_batch_labels",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "dataframe_from_csv",
            vec![Type::String, int.clone()],
            int.clone(),
        ),
        ("dataframe_rows", vec![int.clone()], int.clone()),
        ("dataframe_cols", vec![int.clone()], int.clone()),
        (
            "dataframe_column",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "experiment_start",
            vec![Type::String, Type::String, int.clone()],
            int.clone(),
        ),
        (
            "experiment_set_config",
            vec![int.clone(), Type::String, Type::String],
            unit.clone(),
        ),
        (
            "experiment_log_metric",
            vec![int.clone(), Type::String, float.clone(), int.clone()],
            unit.clone(),
        ),
        (
            "experiment_log_artifact",
            vec![int.clone(), Type::String],
            unit.clone(),
        ),
        (
            "experiment_set_lockfile",
            vec![int.clone(), Type::String],
            unit.clone(),
        ),
        (
            "experiment_set_model_output",
            vec![int.clone(), Type::String],
            unit.clone(),
        ),
        ("experiment_finish", vec![int.clone()], unit.clone()),
        ("experiment_manifest_path", vec![int.clone()], Type::String),
        ("experiment_repro_command", vec![int.clone()], Type::String),
        (
            "experiment_compare_manifests",
            vec![Type::String, Type::String],
            int.clone(),
        ),
        (
            "distributed_session_start",
            vec![Type::String, Type::String, int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "distributed_worker_step",
            vec![int.clone(), int.clone(), int.clone(), float.clone()],
            int.clone(),
        ),
        ("distributed_global_step", vec![int.clone()], int.clone()),
        (
            "distributed_worker_step_count",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "distributed_checkpoint_save",
            vec![int.clone(), Type::String, int.clone()],
            Type::String,
        ),
        ("distributed_resume", vec![Type::String], int.clone()),
        ("distributed_summary", vec![int.clone()], Type::String),
        (
            "distributed_train_multithread",
            vec![
                Type::String,
                Type::String,
                int.clone(),
                int.clone(),
                float.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
            ],
            int.clone(),
        ),
        (
            "distributed_train_tcp",
            vec![
                Type::String,
                Type::String,
                int.clone(),
                int.clone(),
                float.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
            ],
            int.clone(),
        ),
        (
            "distributed_train_dataset_multithread",
            vec![
                int.clone(),
                Type::String,
                int.clone(),
                int.clone(),
                float.clone(),
                int.clone(),
            ],
            int.clone(),
        ),
        (
            "distributed_train_dataset_tcp",
            vec![
                int.clone(),
                Type::String,
                int.clone(),
                int.clone(),
                float.clone(),
                int.clone(),
            ],
            int.clone(),
        ),
        (
            "onnx_export_weights",
            vec![
                Type::String,
                Type::String,
                Type::Applied {
                    name: "List".to_string(),
                    args: vec![int.clone()],
                },
            ],
            Type::String,
        ),
        ("onnx_import_summary", vec![Type::String], Type::String),
        ("onnx_validate", vec![Type::String], int.clone()),
        (
            "onnx_roundtrip",
            vec![Type::String, Type::String],
            Type::String,
        ),
        (
            "embedding_lookup",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "positional_encoding",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "layer_norm",
            vec![int.clone(), int.clone(), int.clone(), float.clone()],
            int.clone(),
        ),
        ("gelu", vec![int.clone()], int.clone()),
        ("swiglu", vec![int.clone(), int.clone()], int.clone()),
        (
            "attention",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("kv_cache_new", vec![int.clone(), int.clone()], int.clone()),
        (
            "kv_cache_append",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("kv_cache_keys", vec![int.clone()], int.clone()),
        ("kv_cache_values", vec![int.clone()], int.clone()),
        ("kv_cache_len", vec![int.clone()], int.clone()),
        (
            "logits_sample",
            vec![int.clone(), float.clone()],
            int.clone(),
        ),
        (
            "logits_sample_seeded",
            vec![int.clone(), int.clone(), float.clone()],
            int.clone(),
        ),
        (
            "generate_ex",
            vec![
                int.clone(),
                int.clone(),
                int.clone(),
                int.clone(),
                float.clone(),
                int.clone(),
                int.clone(),
            ],
            int.clone(),
        ),
        ("tokenizer_wordpiece", vec![Type::String], int.clone()),
        ("tokenizer_load", vec![Type::String], int.clone()),
        (
            "tokenizer_encode",
            vec![int.clone(), Type::String],
            int.clone(),
        ),
        (
            "tokenizer_decode",
            vec![int.clone(), int.clone()],
            Type::String,
        ),
        ("train_bpe", vec![Type::String, int.clone()], int.clone()),
        ("train_wordpiece", vec![Type::String, int.clone()], int.clone()),
        ("tokenizer_vocab", vec![int.clone()], Type::String),
        ("text_embed_model_session", vec![Type::String], int.clone()),
        (
            "text_embed_model",
            vec![int.clone(), int.clone(), Type::String],
            int.clone(),
        ),
        ("embedding_load", vec![Type::String, Type::String], int.clone()),
        ("vector_index_new", vec![int.clone()], int.clone()),
        (
            "vector_index_insert",
            vec![int.clone(), Type::String, int.clone()],
            int.clone(),
        ),
        (
            "vector_index_query",
            vec![int.clone(), int.clone(), int.clone()],
            Type::String,
        ),
        (
            "vector_index_persist",
            vec![int.clone(), Type::String],
            Type::String,
        ),
        ("vector_index_load", vec![Type::String], int.clone()),
        (
            "vector_index_set_metadata",
            vec![int.clone(), Type::String, Type::String],
            Type::Bool,
        ),
        ("vector_index_metrics", vec![int.clone()], Type::String),
        (
            "rag_chunk_text",
            vec![Type::String, int.clone(), int.clone()],
            Type::String,
        ),
        (
            "rag_build_prompt",
            vec![Type::String, Type::String],
            Type::String,
        ),
        (
            "rag_evaluate_answer",
            vec![Type::String, Type::String],
            int.clone(),
        ),
        (
            "metrics_classification",
            vec![int.clone(), int.clone()],
            Type::String,
        ),
        (
            "metrics_regression",
            vec![int.clone(), int.clone()],
            Type::String,
        ),
        (
            "metrics_ranking",
            vec![int.clone(), int.clone(), int.clone()],
            Type::String,
        ),
        (
            "metrics_generation",
            vec![Type::String, Type::String],
            Type::String,
        ),
        (
            "serving_metrics",
            vec![int.clone(), int.clone(), int.clone()],
            Type::String,
        ),
        (
            "evaluation_report",
            vec![
                Type::String,
                Type::String,
                Type::String,
                Type::String,
                Type::String,
                Type::String,
                Type::String,
            ],
            Type::String,
        ),
        (
            "artifact_new",
            vec![Type::String, Type::String, Type::String],
            int.clone(),
        ),
        (
            "artifact_set_metadata",
            vec![int.clone(), Type::String, Type::String],
            bool_ty.clone(),
        ),
        (
            "artifact_add_tensor",
            vec![int.clone(), Type::String, int.clone()],
            bool_ty.clone(),
        ),
        ("artifact_save", vec![int.clone(), Type::String], bool_ty.clone()),
        ("artifact_load", vec![Type::String], int.clone()),
        ("artifact_tensor", vec![int.clone(), Type::String], int.clone()),
        ("artifact_metadata", vec![int.clone(), Type::String], Type::String),
        ("artifact_validate", vec![Type::String], bool_ty.clone()),
        ("artifact_free", vec![int.clone()], unit.clone()),
    ];

    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }

    exports.types.insert(
        "Module".to_string(),
        ExportedType {
            members: vec!["parameters".to_string(), "training".to_string()],
            visibility: ExportVisibility::Public,
            is_enum: false,
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );

    exports
}

pub(crate) fn make_std_concurrent() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "concurrent".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let int = Type::Int;
    let bool_ty = Type::Bool;
    let unit = Type::Unit;

    let fn_int_to_int = Type::Fn {
        params: vec![Type::Int],
        return_type: Box::new(Type::Int),
    };

    let functions = [
        ("task_join", vec![int.clone()], int.clone()),
        (
            "task_spawn_fn",
            vec![fn_int_to_int, int.clone()],
            int.clone(),
        ),
        (
            "task_spawn_batch",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        ("task_join_batch_sum", vec![int.clone()], int.clone()),
        ("task_is_done", vec![int.clone()], bool_ty.clone()),
        ("channel_new", vec![], int.clone()),
        (
            "channel_send",
            vec![int.clone(), int.clone()],
            bool_ty.clone(),
        ),
        ("channel_recv", vec![int.clone()], int.clone()),
        ("channel_len", vec![int.clone()], int.clone()),
        ("channel_close", vec![int.clone()], unit.clone()),
        ("counter_new", vec![int.clone()], int.clone()),
        ("counter_add", vec![int.clone(), int.clone()], int.clone()),
        ("counter_get", vec![int.clone()], int.clone()),
        (
            "pipeline_sum",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("stats_tasks_spawned", vec![], int.clone()),
        ("stats_channels", vec![], int.clone()),
        ("reset", vec![], unit.clone()),
    ];

    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }

    exports
}

pub(crate) fn make_std_serve() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "serve".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let int = Type::Int;
    let bool_ty = Type::Bool;

    let functions = [
        ("server_new", vec![int.clone()], int.clone()),
        ("server_warmup", vec![int.clone()], bool_ty.clone()),
        ("server_is_warm", vec![int.clone()], bool_ty.clone()),
        (
            "server_enqueue",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "server_cancel",
            vec![int.clone(), int.clone()],
            bool_ty.clone(),
        ),
        (
            "server_process_batch",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        ("server_result", vec![int.clone(), int.clone()], int.clone()),
        ("server_pending", vec![int.clone()], int.clone()),
        (
            "server_set_timeout",
            vec![int.clone(), int.clone()],
            bool_ty.clone(),
        ),
        ("server_resident_model", vec![int.clone()], int.clone()),
        (
            "server_benchmark",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "server_set_input_policy",
            vec![int.clone(), int.clone(), int.clone()],
            bool_ty.clone(),
        ),
        (
            "server_set_output_policy",
            vec![int.clone(), int.clone(), int.clone()],
            bool_ty.clone(),
        ),
        (
            "server_set_rate_limit",
            vec![int.clone(), int.clone()],
            bool_ty.clone(),
        ),
        (
            "server_set_fallback",
            vec![int.clone(), int.clone()],
            bool_ty.clone(),
        ),
        ("server_last_diagnostic", vec![int.clone()], Type::String),
        ("server_audit_log", vec![int.clone()], Type::String),
        (
            "server_set_model_version",
            vec![int.clone(), Type::String],
            bool_ty.clone(),
        ),
        (
            "server_monitoring_snapshot",
            vec![int.clone()],
            Type::String,
        ),
        (
            "server_distribution_summary",
            vec![int.clone()],
            Type::String,
        ),
        (
            "drift_check",
            vec![Type::String, Type::String, int.clone()],
            Type::String,
        ),
        (
            "export_monitoring",
            vec![
                int.clone(),
                Type::String,
                Type::String,
                Type::String,
                Type::String,
            ],
            Type::String,
        ),
        (
            "server_register_model_linear",
            vec![int.clone(), int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "server_register_named_model_linear",
            vec![
                int.clone(),
                Type::String,
                int.clone(),
                int.clone(),
                int.clone(),
            ],
            int.clone(),
        ),
        (
            "server_register_model_onnx",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "server_register_named_model_onnx",
            vec![int.clone(), Type::String, int.clone()],
            int.clone(),
        ),
        (
            "server_infer",
            vec![int.clone(), Type::String, int.clone()],
            int.clone(),
        ),
        (
            "server_result_vector",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        ("http_start", vec![int.clone(), int.clone()], int.clone()),
        ("http_stop", vec![int.clone()], bool_ty.clone()),
        ("reset", vec![], Type::Unit),
    ];

    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }

    exports
}

