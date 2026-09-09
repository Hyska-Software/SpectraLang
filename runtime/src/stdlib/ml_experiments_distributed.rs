use super::*;
pub(crate) extern "C" fn std_ml_experiment_start(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(name) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(out_dir) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let manifest_path = std::path::Path::new(&out_dir)
            .join("experiment-manifest.json")
            .to_string_lossy()
            .to_string();
        let reproduction_command = format!(
            "spectralang run <training.spectra> --package-lock spectra.lock --experiment-manifest {}",
            manifest_path
        );
        let handle = with_ml_registry(|registry| {
            registry.experiments.insert(MlExperiment {
                name,
                out_dir,
                seed: args[2],
                configs: Vec::new(),
                metrics: Vec::new(),
                artifacts: Vec::new(),
                lockfile: None,
                model_output: None,
                manifest_path,
                reproduction_command,
                finished: false,
            })
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_experiment_set_config(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(key) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) = ml_read_path_arg(args[2]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let ok = with_ml_registry(|registry| {
            let Some(experiment) = registry.experiments.get_mut(&(args[0] as usize)) else {
                return false;
            };
            if let Some((_, existing)) = experiment
                .configs
                .iter_mut()
                .find(|(existing_key, _)| existing_key == &key)
            {
                *existing = value;
            } else {
                experiment.configs.push((key, value));
            }
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_experiment_log_metric(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 4) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(name) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value = f64::from_bits(args[2] as u64);
        if !value.is_finite() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ok = with_ml_registry(|registry| {
            let Some(experiment) = registry.experiments.get_mut(&(args[0] as usize)) else {
                return false;
            };
            experiment.metrics.push(MlMetricRecord {
                name,
                value,
                step: args[3],
            });
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_experiment_log_artifact(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let artifact = match ml_artifact_record(path) {
            Ok(record) => record,
            Err(code) => return code,
        };
        let ok = with_ml_registry(|registry| {
            let Some(experiment) = registry.experiments.get_mut(&(args[0] as usize)) else {
                return false;
            };
            experiment.artifacts.push(artifact);
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_experiment_set_lockfile(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let lockfile = match ml_artifact_record(path) {
            Ok(record) => record,
            Err(code) => return code,
        };
        let ok = with_ml_registry(|registry| {
            let Some(experiment) = registry.experiments.get_mut(&(args[0] as usize)) else {
                return false;
            };
            experiment.lockfile = Some(lockfile);
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_experiment_set_model_output(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let output = match ml_artifact_record(path) {
            Ok(record) => record,
            Err(code) => return code,
        };
        let ok = with_ml_registry(|registry| {
            let Some(experiment) = registry.experiments.get_mut(&(args[0] as usize)) else {
                return false;
            };
            experiment.model_output = Some(output);
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_experiment_finish(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((out_dir, manifest_path, payload)) = with_ml_registry(|registry| {
            let experiment = registry.experiments.get_mut(&(args[0] as usize))?;
            experiment.finished = true;
            Some((
                experiment.out_dir.clone(),
                experiment.manifest_path.clone(),
                ml_experiment_manifest_json(experiment),
            ))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if std::fs::create_dir_all(&out_dir).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        if std::fs::write(&manifest_path, payload.as_bytes()).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_experiment_manifest_path(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = with_ml_registry(|registry| {
            registry
                .experiments
                .get(&(args[0] as usize))
                .map(|experiment| experiment.manifest_path.clone())
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, alloc_spectra_string(&path))
    }
}

pub(crate) extern "C" fn std_ml_experiment_repro_command(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(command) = with_ml_registry(|registry| {
            registry
                .experiments
                .get(&(args[0] as usize))
                .map(|experiment| experiment.reproduction_command.clone())
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, alloc_spectra_string(&command))
    }
}

pub(crate) extern "C" fn std_ml_experiment_compare_manifests(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(left_path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(right_path) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let left = match std::fs::read_to_string(&left_path) {
            Ok(value) => value,
            Err(_) => return HOST_STATUS_NOT_FOUND,
        };
        let right = match std::fs::read_to_string(&right_path) {
            Ok(value) => value,
            Err(_) => return HOST_STATUS_NOT_FOUND,
        };
        tensor_result(
            ctx_ref,
            if ml_compare_manifest_payloads(&left, &right) {
                1
            } else {
                0
            },
        )
    }
}

pub(crate) extern "C" fn std_ml_distributed_session_start(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 4) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(name) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(out_dir) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let worker_count = args[2];
        if worker_count <= 0 || worker_count > 1024 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let worker_count = worker_count as usize;
        let seed = args[3];
        let handle = with_ml_registry(|registry| {
            let workers = (0..worker_count)
                .map(|worker_id| MlDistributedWorker {
                    worker_id,
                    step_count: 0,
                    sample_count: 0,
                    accumulator: 0.0,
                    active: true,
                })
                .collect();
            registry.distributed_sessions.insert(MlDistributedSession {
                name,
                out_dir,
                worker_count,
                seed,
                global_step: 0,
                interrupted_worker: None,
                workers,
                last_checkpoint_path: None,
                topology: "multi-thread".to_string(),
                last_loss: 0.0,
            })
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_distributed_global_step(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(global_step) = with_ml_registry(|registry| {
            let session = registry.distributed_sessions.get_mut(&(args[0] as usize))?;
            if session
                .workers
                .iter()
                .all(|worker| worker.step_count > session.global_step)
            {
                session.global_step += 1;
            }
            Some(session.global_step)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, global_step)
    }
}

pub(crate) extern "C" fn std_ml_distributed_worker_step_count(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(step_count) = with_ml_registry(|registry| {
            let session = registry.distributed_sessions.get(&(args[0] as usize))?;
            Some(session.workers.get(args[1] as usize)?.step_count)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, step_count)
    }
}

pub(crate) extern "C" fn std_ml_distributed_checkpoint_save(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let interrupted_worker = if args[2] < 0 {
            None
        } else {
            Some(args[2] as usize)
        };
        let Some((out_dir, payload)) = with_ml_registry(|registry| {
            let session = registry.distributed_sessions.get_mut(&(args[0] as usize))?;
            if let Some(worker_id) = interrupted_worker {
                let worker = session.workers.get_mut(worker_id)?;
                worker.active = false;
                session.interrupted_worker = Some(worker_id);
            } else {
                session.interrupted_worker = None;
            }
            session.last_checkpoint_path = Some(path.clone());
            Some((
                session.out_dir.clone(),
                ml_distributed_session_json(session),
            ))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if std::fs::create_dir_all(&out_dir).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return HOST_STATUS_INTERNAL_ERROR;
            }
        }
        if std::fs::write(&path, payload.as_bytes()).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        tensor_result(ctx_ref, alloc_spectra_string(&path))
    }
}

pub(crate) extern "C" fn std_ml_distributed_resume(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let payload = match std::fs::read_to_string(&path) {
            Ok(value) => value,
            Err(_) => return HOST_STATUS_NOT_FOUND,
        };
        let Some(mut session) = ml_distributed_session_from_checkpoint(&payload, path) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        for worker in &mut session.workers {
            worker.active = true;
        }
        session.interrupted_worker = None;
        let handle = with_ml_registry(|registry| {
            registry.distributed_sessions.insert(session)
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_distributed_summary(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(summary) = with_ml_registry(|registry| {
            registry
                .distributed_sessions
                .get(&(args[0] as usize))
                .map(ml_distributed_summary_json)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, alloc_spectra_string(&summary))
    }
}


// ── DistTCP ──────────────────────────────────────────────────────────────────
// Real data-parallel training entry points. Both run actual forward/backward
// passes on OS-thread workers over disjoint shards and ALLREDUCE the gradients;
// they differ only in transport (shared-memory barriers vs TCP loopback).

pub(crate) extern "C" fn std_ml_distributed_train_multithread(ctx: *mut SpectraHostCallContext) -> i32 {
    ml_distributed_train(ctx, ML_DISTRIBUTED_TOPOLOGY_MULTITHREAD, |spec| {
        dist_run_multithread(spec)
    })
}

pub(crate) extern "C" fn std_ml_distributed_train_tcp(ctx: *mut SpectraHostCallContext) -> i32 {
    ml_distributed_train(ctx, ML_DISTRIBUTED_TOPOLOGY_TCP, |spec| dist_run_tcp(spec))
}

pub(crate) extern "C" fn std_ml_distributed_train_dataset_multithread(
    ctx: *mut SpectraHostCallContext,
) -> i32 {
    ml_distributed_train_dataset(ctx, ML_DISTRIBUTED_TOPOLOGY_MULTITHREAD, |spec| {
        dist_run_multithread(spec)
    })
}

pub(crate) extern "C" fn std_ml_distributed_train_dataset_tcp(
    ctx: *mut SpectraHostCallContext,
) -> i32 {
    ml_distributed_train_dataset(ctx, ML_DISTRIBUTED_TOPOLOGY_TCP, |spec| {
        dist_run_tcp(spec)
    })
}

/// Dataset-backed twin of [`ml_distributed_train`]: identical runners,
/// session recording, and outcome shape, but shards slice a caller dataset
/// instead of synthesizing a fixed linear problem.
pub(crate) fn ml_distributed_train_dataset<F>(
    ctx: *mut SpectraHostCallContext,
    topology: &str,
    run: F,
) -> i32
where
    F: FnOnce(&DistTrainSpec) -> Result<DistRunOutcome, i32>,
{
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 6) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let dataset = args[0];
        if dataset <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(out_dir) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let lr = f64::from_bits(args[4] as u64);
        let spec = match dist_parse_dataset_spec(dataset as usize, args[2], args[3], lr, args[5])
        {
            Ok(spec) => spec,
            Err(code) => return code,
        };
        if std::fs::create_dir_all(&out_dir).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        match run(&spec) {
            Ok(outcome) => {
                let name = format!("distributed-dataset-{dataset}");
                let session =
                    dist_outcome_to_session(name, out_dir, &spec, topology, outcome);
                let handle = with_ml_registry(|registry| {
                    registry.distributed_sessions.insert(session)
                });
                tensor_result(ctx_ref, handle as SpectraHostValue)
            }
            Err(code) => code,
        }
    }
}

pub(crate) fn ml_distributed_train<F>(
    ctx: *mut SpectraHostCallContext,
    topology: &str,
    run: F,
) -> i32
where
    F: FnOnce(&DistTrainSpec) -> Result<DistRunOutcome, i32>,
{
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 8) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(name) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(out_dir) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Ok(spec) = dist_parse_spec(args) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if std::fs::create_dir_all(&out_dir).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        match run(&spec) {
            Ok(outcome) => {
                let session =
                    dist_outcome_to_session(name, out_dir, &spec, topology, outcome);
                let handle = with_ml_registry(|registry| {
                    registry.distributed_sessions.insert(session)
                });
                tensor_result(ctx_ref, handle as SpectraHostValue)
            }
            Err(code) => code,
        }
    }
}
