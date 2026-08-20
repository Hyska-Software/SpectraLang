fn lookup_std_host_group_tensor_ml(module: &str, function: &str) -> Option<HostFunctionDescriptor> {
    match (module, function) {
            ("tensor", "vector_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.tensor.full_f",
                return_type: IRType::Tensor {
                    dtype: Box::new(IRType::Float),
                    rank: Some(1),
                    dims: None,
                    layout: None,
                    device: None,
                },
                returns_value: true,
            }),
            ("tensor", "matrix_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.tensor.full2_f",
                return_type: IRType::Tensor {
                    dtype: Box::new(IRType::Float),
                    rank: Some(2),
                    dims: None,
                    layout: None,
                    device: None,
                },
                returns_value: true,
            }),
            ("tensor", "zeros") => Some(host_int("spectra.std.tensor.zeros")),
            ("tensor", "ones") => Some(host_int("spectra.std.tensor.ones")),
            ("tensor", "full") => Some(host_int("spectra.std.tensor.full")),
            ("tensor", "full_f") => Some(host_int("spectra.std.tensor.full_f")),
            ("tensor", "arange") => Some(host_int("spectra.std.tensor.arange")),
            ("tensor", "zeros2") => Some(host_int("spectra.std.tensor.zeros2")),
            ("tensor", "ones2") => Some(host_int("spectra.std.tensor.ones2")),
            ("tensor", "full2") => Some(host_int("spectra.std.tensor.full2")),
            ("tensor", "full2_f") => Some(host_int("spectra.std.tensor.full2_f")),
            ("tensor", "len") => Some(host_int("spectra.std.tensor.len")),
            ("tensor", "rank") => Some(host_int("spectra.std.tensor.rank")),
            ("tensor", "dim") => Some(host_int("spectra.std.tensor.dim")),
            ("tensor", "rows") => Some(host_int("spectra.std.tensor.rows")),
            ("tensor", "cols") => Some(host_int("spectra.std.tensor.cols")),
            ("tensor", "is_valid") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.tensor.is_valid",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("tensor", "get") => Some(host_int("spectra.std.tensor.get")),
            ("tensor", "get_f") => Some(host_float("spectra.std.tensor.get_f")),
            ("tensor", "set") => Some(host_void("spectra.std.tensor.set")),
            ("tensor", "set_f") => Some(host_void("spectra.std.tensor.set_f")),
            ("tensor", "get2") => Some(host_int("spectra.std.tensor.get2")),
            ("tensor", "get2_f") => Some(host_float("spectra.std.tensor.get2_f")),
            ("tensor", "set2") => Some(host_void("spectra.std.tensor.set2")),
            ("tensor", "set2_f") => Some(host_void("spectra.std.tensor.set2_f")),
            ("tensor", "reshape") => Some(host_int("spectra.std.tensor.reshape")),
            ("tensor", "flatten") => Some(host_int("spectra.std.tensor.flatten")),
            ("tensor", "permute") => Some(host_int("spectra.std.tensor.permute")),
            ("tensor", "slice") => Some(host_int("spectra.std.tensor.slice")),
            ("tensor", "concat") => Some(host_int("spectra.std.tensor.concat")),
            ("tensor", "stack") => Some(host_int("spectra.std.tensor.stack")),
            ("tensor", "add") => Some(host_int("spectra.std.tensor.add")),
            ("tensor", "sub") => Some(host_int("spectra.std.tensor.sub")),
            ("tensor", "mul") => Some(host_tensor_dynamic("spectra.std.tensor.mul")),
            ("tensor", "div") => Some(host_int("spectra.std.tensor.div")),
            ("tensor", "sum") => Some(host_int("spectra.std.tensor.sum")),
            ("tensor", "sum_f") => Some(host_float("spectra.std.tensor.sum_f")),
            ("tensor", "sum_t") => Some(host_tensor_rank0("spectra.std.tensor.sum_t")),
            ("tensor", "mean_f") => Some(host_float("spectra.std.tensor.mean_f")),
            ("tensor", "mean_t") => Some(host_int("spectra.std.tensor.mean_t")),
            ("tensor", "max") => Some(host_int("spectra.std.tensor.max")),
            ("tensor", "min") => Some(host_int("spectra.std.tensor.min")),
            ("tensor", "argmax") => Some(host_int("spectra.std.tensor.argmax")),
            ("tensor", "matmul") => Some(host_int("spectra.std.tensor.matmul")),
            ("tensor", "matmul_batched") => Some(host_int("spectra.std.tensor.matmul_batched")),
            ("tensor", "transpose") => Some(host_int("spectra.std.tensor.transpose")),
            ("tensor", "dot") => Some(host_int("spectra.std.tensor.dot")),
            ("tensor", "dot_t") => Some(host_int("spectra.std.tensor.dot_t")),
            ("tensor", "neg") => Some(host_int("spectra.std.tensor.neg")),
            ("tensor", "exp_f") => Some(host_int("spectra.std.tensor.exp_f")),
            ("tensor", "log_f") => Some(host_int("spectra.std.tensor.log_f")),
            ("tensor", "sqrt_f") => Some(host_int("spectra.std.tensor.sqrt_f")),
            ("tensor", "relu") => Some(host_int("spectra.std.tensor.relu")),
            ("tensor", "sigmoid_f") => Some(host_int("spectra.std.tensor.sigmoid_f")),
            ("tensor", "tanh_f") => Some(host_int("spectra.std.tensor.tanh_f")),
            ("tensor", "seed") => Some(host_void("spectra.std.tensor.seed")),
            ("tensor", "uniform") => Some(host_int("spectra.std.tensor.uniform")),
            ("tensor", "uniform_f") => Some(host_int("spectra.std.tensor.uniform_f")),
            ("tensor", "normal_f") => Some(host_int("spectra.std.tensor.normal_f")),
            ("tensor", "bernoulli") => Some(host_int("spectra.std.tensor.bernoulli")),
            ("tensor", "categorical") => Some(host_int("spectra.std.tensor.categorical")),
            ("tensor", "set_deterministic_mode") => {
                Some(host_int("spectra.std.tensor.set_deterministic_mode"))
            }
            ("tensor", "deterministic_mode") => {
                Some(host_int("spectra.std.tensor.deterministic_mode"))
            }
            ("tensor", "tolerance_abs") => Some(host_float("spectra.std.tensor.tolerance_abs")),
            ("tensor", "tolerance_rel") => Some(host_float("spectra.std.tensor.tolerance_rel")),
            ("tensor", "device") => Some(host_int("spectra.std.tensor.device")),
            ("tensor", "device_available") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.tensor.device_available",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("tensor", "device_status") => Some(host_int("spectra.std.tensor.device_status")),
            ("tensor", "to_device") => Some(host_int("spectra.std.tensor.to_device")),
            ("tensor", "cpu") => Some(host_int("spectra.std.tensor.cpu")),
            ("tensor", "sync") => Some(host_void("spectra.std.tensor.sync")),
            ("tensor", "precision") => Some(host_int("spectra.std.tensor.precision")),
            ("tensor", "to_precision") => Some(host_int("spectra.std.tensor.to_precision")),
            ("tensor", "stats_allocations") => {
                Some(host_int("spectra.std.tensor.stats_allocations"))
            }
            ("tensor", "stats_active") => Some(host_int("spectra.std.tensor.stats_active")),
            ("tensor", "stats_peak_bytes") => Some(host_int("spectra.std.tensor.stats_peak_bytes")),
            ("tensor", "stats_reused_buffers") => {
                Some(host_int("spectra.std.tensor.stats_reused_buffers"))
            }
            ("tensor", "stats_pool_hits") => Some(host_int("spectra.std.tensor.stats_pool_hits")),
            ("tensor", "stats_pool_misses") => {
                Some(host_int("spectra.std.tensor.stats_pool_misses"))
            }
            ("tensor", "stats_active_bytes") => {
                Some(host_int("spectra.std.tensor.stats_active_bytes"))
            }
            ("tensor", "stats_scratch_reuses") => {
                Some(host_int("spectra.std.tensor.stats_scratch_reuses"))
            }
            ("tensor", "kernel_strategy") => Some(host_int("spectra.std.tensor.kernel_strategy")),
            ("tensor", "stats_kernel_ops") => Some(host_int("spectra.std.tensor.stats_kernel_ops")),
            ("tensor", "stats_kernel_elements") => {
                Some(host_int("spectra.std.tensor.stats_kernel_elements"))
            }
            ("tensor", "stats_device_transfers") => {
                Some(host_int("spectra.std.tensor.stats_device_transfers"))
            }
            ("tensor", "stats_gpu_kernel_ops") => {
                Some(host_int("spectra.std.tensor.stats_gpu_kernel_ops"))
            }
            ("tensor", "stats_cpu_fallbacks") => {
                Some(host_int("spectra.std.tensor.stats_cpu_fallbacks"))
            }
            ("tensor", "stats_device_resident_tensors") => {
                Some(host_int("spectra.std.tensor.stats_device_resident_tensors"))
            }
            ("tensor", "stats_gpu_backward_ops") => {
                Some(host_int("spectra.std.tensor.stats_gpu_backward_ops"))
            }
            ("tensor", "stats_graph_nodes") => {
                Some(host_int("spectra.std.tensor.stats_graph_nodes"))
            }
            ("tensor", "stats_lifetime_records") => {
                Some(host_int("spectra.std.tensor.stats_lifetime_records"))
            }
            ("tensor", "stats_released_lifetimes") => {
                Some(host_int("spectra.std.tensor.stats_released_lifetimes"))
            }
            ("tensor", "stats_allocation_sites") => {
                Some(host_int("spectra.std.tensor.stats_allocation_sites"))
            }
            ("tensor", "stats_reuse_rate_per_mille") => {
                Some(host_int("spectra.std.tensor.stats_reuse_rate_per_mille"))
            }
            ("tensor", "memory_report") => Some(host_string("spectra.std.tensor.memory_report")),
            ("tensor", "reset_stats") => Some(host_void("spectra.std.tensor.reset_stats")),
            ("tensor", "requires_grad") => {
                Some(host_tensor_dynamic("spectra.std.tensor.requires_grad"))
            }
            ("tensor", "diff") => Some(host_void("spectra.std.tensor.backward")),
            ("tensor", "backward") => Some(host_void("spectra.std.tensor.backward")),
            ("tensor", "grad") => Some(host_tensor_dynamic("spectra.std.tensor.grad")),
            ("tensor", "zero_grad") => Some(host_void("spectra.std.tensor.zero_grad")),
            ("tensor", "set_grad_enabled") => {
                Some(host_void("spectra.std.tensor.set_grad_enabled"))
            }
            ("tensor", "grad_enabled") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.tensor.grad_enabled",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("tensor", "free") => Some(host_void("spectra.std.tensor.free")),
            ("tensor", "free_all") => Some(host_int("spectra.std.tensor.free_all")),
            ("tensor", "refill") => Some(host_void("spectra.std.tensor.refill")),
            // ── std.ml ───────────────────────────────────────────────────
            ("ml", "module_new") => Some(host_int("spectra.std.ml.module_new")),
            ("ml", "module_add_parameter") => {
                Some(host_void("spectra.std.ml.module_add_parameter"))
            }
            ("ml", "module_parameter_count") => {
                Some(host_int("spectra.std.ml.module_parameter_count"))
            }
            ("ml", "module_parameter") => Some(host_int("spectra.std.ml.module_parameter")),
            ("ml", "module_set_training") => Some(host_void("spectra.std.ml.module_set_training")),
            ("ml", "module_is_training") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.ml.module_is_training",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("ml", "linear") => Some(host_int("spectra.std.ml.linear")),
            ("ml", "conv2d") => Some(host_int("spectra.std.ml.conv2d")),
            ("ml", "dropout") => Some(host_int("spectra.std.ml.dropout")),
            ("ml", "max_pool2d") => Some(host_int("spectra.std.ml.max_pool2d")),
            ("ml", "mse_loss") => Some(host_int("spectra.std.ml.mse_loss")),
            ("ml", "bce_loss") => Some(host_int("spectra.std.ml.bce_loss")),
            ("ml", "cross_entropy_loss") => Some(host_int("spectra.std.ml.cross_entropy_loss")),
            ("ml", "nll_loss") => Some(host_int("spectra.std.ml.nll_loss")),
            ("ml", "sgd_step") => Some(host_void("spectra.std.ml.sgd_step")),
            ("ml", "sgd_momentum_step") => Some(host_void("spectra.std.ml.sgd_momentum_step")),
            ("ml", "adam_step") => Some(host_void("spectra.std.ml.adam_step")),
            ("ml", "adamw_step") => Some(host_void("spectra.std.ml.adamw_step")),
            ("ml", "exp_lr") => Some(host_float("spectra.std.ml.exp_lr")),
            ("ml", "unscale_grad") => Some(host_void("spectra.std.ml.unscale_grad")),
            ("ml", "dataset_from_tensors") => Some(host_int("spectra.std.ml.dataset_from_tensors")),
            ("ml", "dataset_from_csv") => Some(host_int("spectra.std.ml.dataset_from_csv")),
            ("ml", "dataset_from_jsonl") => Some(host_int("spectra.std.ml.dataset_from_jsonl")),
            ("ml", "dataset_from_npy") => Some(host_int("spectra.std.ml.dataset_from_npy")),
            ("ml", "dataset_from_directory") => {
                Some(host_int("spectra.std.ml.dataset_from_directory"))
            }
            ("ml", "dataset_len") => Some(host_int("spectra.std.ml.dataset_len")),
            ("ml", "dataset_map_features") => Some(host_int("spectra.std.ml.dataset_map_features")),
            ("ml", "dataset_filter_label_min") => {
                Some(host_int("spectra.std.ml.dataset_filter_label_min"))
            }
            ("ml", "dataset_train_split") => Some(host_int("spectra.std.ml.dataset_train_split")),
            ("ml", "dataset_test_split") => Some(host_int("spectra.std.ml.dataset_test_split")),
            ("ml", "dataloader_new") => Some(host_int("spectra.std.ml.dataloader_new")),
            ("ml", "dataloader_batch_count") => {
                Some(host_int("spectra.std.ml.dataloader_batch_count"))
            }
            ("ml", "dataloader_batch_features") => {
                Some(host_int("spectra.std.ml.dataloader_batch_features"))
            }
            ("ml", "dataloader_batch_labels") => {
                Some(host_int("spectra.std.ml.dataloader_batch_labels"))
            }
            ("ml", "dataframe_from_csv") => Some(host_int("spectra.std.ml.dataframe_from_csv")),
            ("ml", "dataframe_rows") => Some(host_int("spectra.std.ml.dataframe_rows")),
            ("ml", "dataframe_cols") => Some(host_int("spectra.std.ml.dataframe_cols")),
            ("ml", "dataframe_column") => Some(host_int("spectra.std.ml.dataframe_column")),
            ("ml", "experiment_start") => Some(host_int("spectra.std.ml.experiment_start")),
            ("ml", "experiment_set_config") => {
                Some(host_void("spectra.std.ml.experiment_set_config"))
            }
            ("ml", "experiment_log_metric") => {
                Some(host_void("spectra.std.ml.experiment_log_metric"))
            }
            ("ml", "experiment_log_artifact") => {
                Some(host_void("spectra.std.ml.experiment_log_artifact"))
            }
            ("ml", "experiment_set_lockfile") => {
                Some(host_void("spectra.std.ml.experiment_set_lockfile"))
            }
            ("ml", "experiment_set_model_output") => {
                Some(host_void("spectra.std.ml.experiment_set_model_output"))
            }
            ("ml", "experiment_finish") => Some(host_void("spectra.std.ml.experiment_finish")),
            ("ml", "experiment_manifest_path") => {
                Some(host_string("spectra.std.ml.experiment_manifest_path"))
            }
            ("ml", "experiment_repro_command") => {
                Some(host_string("spectra.std.ml.experiment_repro_command"))
            }
            ("ml", "experiment_compare_manifests") => {
                Some(host_int("spectra.std.ml.experiment_compare_manifests"))
            }
            ("ml", "distributed_session_start") => {
                Some(host_int("spectra.std.ml.distributed_session_start"))
            }
            ("ml", "distributed_worker_step") => {
                Some(host_int("spectra.std.ml.distributed_worker_step"))
            }
            ("ml", "distributed_global_step") => {
                Some(host_int("spectra.std.ml.distributed_global_step"))
            }
            ("ml", "distributed_worker_step_count") => {
                Some(host_int("spectra.std.ml.distributed_worker_step_count"))
            }
            ("ml", "distributed_checkpoint_save") => {
                Some(host_string("spectra.std.ml.distributed_checkpoint_save"))
            }
            ("ml", "distributed_resume") => Some(host_int("spectra.std.ml.distributed_resume")),
            ("ml", "distributed_summary") => {
                Some(host_string("spectra.std.ml.distributed_summary"))
            }
            ("ml", "onnx_export") => Some(host_string("spectra.std.ml.onnx_export")),
            ("ml", "onnx_import_summary") => {
                Some(host_string("spectra.std.ml.onnx_import_summary"))
            }
            ("ml", "onnx_validate") => Some(host_int("spectra.std.ml.onnx_validate")),
            ("ml", "onnx_roundtrip") => Some(host_string("spectra.std.ml.onnx_roundtrip")),
            ("ml", "embedding_lookup") => Some(host_int("spectra.std.ml.embedding_lookup")),
            ("ml", "positional_encoding") => Some(host_int("spectra.std.ml.positional_encoding")),
            ("ml", "layer_norm") => Some(host_int("spectra.std.ml.layer_norm")),
            ("ml", "gelu") => Some(host_int("spectra.std.ml.gelu")),
            ("ml", "swiglu") => Some(host_int("spectra.std.ml.swiglu")),
            ("ml", "attention") => Some(host_int("spectra.std.ml.attention")),
            ("ml", "kv_cache_new") => Some(host_int("spectra.std.ml.kv_cache_new")),
            ("ml", "kv_cache_append") => Some(host_int("spectra.std.ml.kv_cache_append")),
            ("ml", "kv_cache_keys") => Some(host_int("spectra.std.ml.kv_cache_keys")),
            ("ml", "kv_cache_values") => Some(host_int("spectra.std.ml.kv_cache_values")),
            ("ml", "kv_cache_len") => Some(host_int("spectra.std.ml.kv_cache_len")),
            ("ml", "logits_sample") => Some(host_int("spectra.std.ml.logits_sample")),
            ("ml", "tokenizer_wordpiece") => Some(host_int("spectra.std.ml.tokenizer_wordpiece")),
            ("ml", "tokenizer_load") => Some(host_int("spectra.std.ml.tokenizer_load")),
            ("ml", "tokenizer_encode") => Some(host_int("spectra.std.ml.tokenizer_encode")),
            ("ml", "tokenizer_decode") => Some(host_string("spectra.std.ml.tokenizer_decode")),
            ("ml", "text_embed") => Some(host_int("spectra.std.ml.text_embed")),
            ("ml", "embedding_load") => Some(host_int("spectra.std.ml.embedding_load")),
            ("ml", "vector_index_new") => Some(host_int("spectra.std.ml.vector_index_new")),
            ("ml", "vector_index_insert") => Some(host_int("spectra.std.ml.vector_index_insert")),
            ("ml", "vector_index_query") => Some(host_string("spectra.std.ml.vector_index_query")),
            ("ml", "vector_index_persist") => {
                Some(host_string("spectra.std.ml.vector_index_persist"))
            }
            ("ml", "vector_index_load") => Some(host_int("spectra.std.ml.vector_index_load")),
            ("ml", "vector_index_set_metadata") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.ml.vector_index_set_metadata",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("ml", "vector_index_metrics") => Some(host_string("spectra.std.ml.vector_index_metrics")),
            ("ml", "rag_chunk_text") => Some(host_string("spectra.std.ml.rag_chunk_text")),
            ("ml", "rag_build_prompt") => Some(host_string("spectra.std.ml.rag_build_prompt")),
            ("ml", "rag_evaluate_answer") => Some(host_int("spectra.std.ml.rag_evaluate_answer")),
            ("ml", "metrics_classification") => {
                Some(host_string("spectra.std.ml.metrics_classification"))
            }
            ("ml", "metrics_regression") => Some(host_string("spectra.std.ml.metrics_regression")),
            ("ml", "metrics_ranking") => Some(host_string("spectra.std.ml.metrics_ranking")),
            ("ml", "metrics_generation") => Some(host_string("spectra.std.ml.metrics_generation")),
            ("ml", "serving_metrics") => Some(host_string("spectra.std.ml.serving_metrics")),
            ("ml", "evaluation_report") => Some(host_string("spectra.std.ml.evaluation_report")),
            ("ml", "artifact_new") => Some(host_int("spectra.std.ml.artifact_new")),
            ("ml", "artifact_set_metadata") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.ml.artifact_set_metadata",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("ml", "artifact_add_tensor") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.ml.artifact_add_tensor",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("ml", "artifact_save") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.ml.artifact_save",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("ml", "artifact_load") => Some(host_int("spectra.std.ml.artifact_load")),
            ("ml", "artifact_tensor") => Some(host_int("spectra.std.ml.artifact_tensor")),
            ("ml", "artifact_metadata") => Some(host_string("spectra.std.ml.artifact_metadata")),
            ("ml", "artifact_validate") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.ml.artifact_validate",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("ml", "artifact_free") => Some(host_void("spectra.std.ml.artifact_free")),
            // ── std.concurrent ───────────────────────────────────────────
            ("concurrent", "task_spawn") => Some(host_int("spectra.std.concurrent.task_spawn")),
            ("concurrent", "task_join") => Some(host_int("spectra.std.concurrent.task_join")),
            ("concurrent", "task_spawn_batch") => {
                Some(host_int("spectra.std.concurrent.task_spawn_batch"))
            }
            ("concurrent", "task_join_batch_sum") => {
                Some(host_int("spectra.std.concurrent.task_join_batch_sum"))
            }
            ("concurrent", "task_is_done") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.concurrent.task_is_done",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("concurrent", "channel_new") => Some(host_int("spectra.std.concurrent.channel_new")),
            ("concurrent", "channel_send") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.concurrent.channel_send",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("concurrent", "channel_recv") => Some(host_int("spectra.std.concurrent.channel_recv")),
            ("concurrent", "channel_len") => Some(host_int("spectra.std.concurrent.channel_len")),
            ("concurrent", "channel_close") => {
                Some(host_void("spectra.std.concurrent.channel_close"))
            }
            ("concurrent", "counter_new") => Some(host_int("spectra.std.concurrent.counter_new")),
            ("concurrent", "counter_add") => Some(host_int("spectra.std.concurrent.counter_add")),
            ("concurrent", "counter_get") => Some(host_int("spectra.std.concurrent.counter_get")),
            ("concurrent", "pipeline_sum") => Some(host_int("spectra.std.concurrent.pipeline_sum")),
            ("concurrent", "stats_tasks_spawned") => {
                Some(host_int("spectra.std.concurrent.stats_tasks_spawned"))
            }
            ("concurrent", "stats_channels") => {
                Some(host_int("spectra.std.concurrent.stats_channels"))
            }
            ("concurrent", "reset") => Some(host_void("spectra.std.concurrent.reset")),
            // ── std.serve ────────────────────────────────────────────────
            ("serve", "server_new") => Some(host_int("spectra.std.serve.server_new")),
            ("serve", "server_warmup") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.serve.server_warmup",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("serve", "server_is_warm") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.serve.server_is_warm",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("serve", "server_enqueue") => Some(host_int("spectra.std.serve.server_enqueue")),
            ("serve", "server_cancel") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.serve.server_cancel",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("serve", "server_process_batch") => {
                Some(host_int("spectra.std.serve.server_process_batch"))
            }
            ("serve", "server_result") => Some(host_int("spectra.std.serve.server_result")),
            ("serve", "server_pending") => Some(host_int("spectra.std.serve.server_pending")),
            ("serve", "server_set_timeout") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.serve.server_set_timeout",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("serve", "server_resident_model") => {
                Some(host_int("spectra.std.serve.server_resident_model"))
            }
            ("serve", "server_benchmark") => Some(host_int("spectra.std.serve.server_benchmark")),
            ("serve", "server_set_input_policy") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.serve.server_set_input_policy",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("serve", "server_set_output_policy") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.serve.server_set_output_policy",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("serve", "server_set_rate_limit") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.serve.server_set_rate_limit",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("serve", "server_set_fallback") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.serve.server_set_fallback",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("serve", "server_last_diagnostic") => {
                Some(host_string("spectra.std.serve.server_last_diagnostic"))
            }
            ("serve", "server_audit_log") => {
                Some(host_string("spectra.std.serve.server_audit_log"))
            }
            ("serve", "server_set_model_version") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.serve.server_set_model_version",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("serve", "server_monitoring_snapshot") => {
                Some(host_string("spectra.std.serve.server_monitoring_snapshot"))
            }
            ("serve", "server_distribution_summary") => {
                Some(host_string("spectra.std.serve.server_distribution_summary"))
            }
            ("serve", "drift_check") => Some(host_string("spectra.std.serve.drift_check")),
            ("serve", "export_monitoring") => {
                Some(host_string("spectra.std.serve.export_monitoring"))
            }
            ("serve", "reset") => Some(host_void("spectra.std.serve.reset")),
        _ => None,
    }
}
