use super::*;
pub(crate) extern "C" fn std_ml_metrics_classification(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(expected) = ml_tensor_int_data(args[0] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(predicted) = ml_tensor_int_data(args[1] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if expected.is_empty() || expected.len() != predicted.len() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }

        let mut correct = 0usize;
        let mut tp = 0usize;
        let mut fp = 0usize;
        let mut fn_count = 0usize;
        let positives = expected.iter().filter(|&&value| value == 1).count();
        let negatives = expected.len().saturating_sub(positives);
        let mut auc_pairs = 0usize;
        let mut auc_wins = 0.0f64;

        for (&actual, &pred) in expected.iter().zip(predicted.iter()) {
            if actual == pred {
                correct += 1;
            }
            match (actual == 1, pred == 1) {
                (true, true) => tp += 1,
                (false, true) => fp += 1,
                (true, false) => fn_count += 1,
                (false, false) => {}
            }
        }
        for (i, &actual_i) in expected.iter().enumerate() {
            if actual_i != 1 {
                continue;
            }
            for (j, &actual_j) in expected.iter().enumerate() {
                if actual_j == 1 {
                    continue;
                }
                auc_pairs += 1;
                let score_i = predicted[i] as f64;
                let score_j = predicted[j] as f64;
                if score_i > score_j {
                    auc_wins += 1.0;
                } else if (score_i - score_j).abs() < f64::EPSILON {
                    auc_wins += 0.5;
                }
            }
        }

        let accuracy = correct as f64 / expected.len() as f64;
        let precision = if tp + fp == 0 {
            0.0
        } else {
            tp as f64 / (tp + fp) as f64
        };
        let recall = if tp + fn_count == 0 {
            0.0
        } else {
            tp as f64 / (tp + fn_count) as f64
        };
        let f1 = if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        };
        let roc_auc_baseline = if positives == 0 || negatives == 0 || auc_pairs == 0 {
            1.0
        } else {
            auc_wins / auc_pairs as f64
        };
        let payload = ml_metrics_json(
            "classification",
            &[
                ("count", expected.len().to_string()),
                ("accuracy", ml_float_json(accuracy)),
                ("precision", ml_float_json(precision)),
                ("recall", ml_float_json(recall)),
                ("f1", ml_float_json(f1)),
                ("roc_auc_baseline", ml_float_json(roc_auc_baseline)),
                ("true_positive", tp.to_string()),
                ("false_positive", fp.to_string()),
                ("false_negative", fn_count.to_string()),
            ],
        );
        tensor_result(ctx_ref, alloc_spectra_string(&payload))
    }
}

pub(crate) extern "C" fn std_ml_metrics_regression(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((_, expected, _)) = ml_tensor_float_data(args[0] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((_, predicted, _)) = ml_tensor_float_data(args[1] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if expected.is_empty() || expected.len() != predicted.len() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let mut squared = 0.0;
        let mut absolute = 0.0;
        for (&actual, &pred) in expected.iter().zip(predicted.iter()) {
            let error = pred - actual;
            squared += error * error;
            absolute += error.abs();
        }
        let n = expected.len() as f64;
        let mse = squared / n;
        let payload = ml_metrics_json(
            "regression",
            &[
                ("count", expected.len().to_string()),
                ("mse", ml_float_json(mse)),
                ("mae", ml_float_json(absolute / n)),
                ("rmse", ml_float_json(mse.sqrt())),
            ],
        );
        tensor_result(ctx_ref, alloc_spectra_string(&payload))
    }
}

pub(crate) extern "C" fn std_ml_metrics_ranking(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(relevance) = ml_tensor_int_data(args[0] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((_, scores, _)) = ml_tensor_float_data(args[1] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let top_k = args[2] as usize;
        if relevance.is_empty() || relevance.len() != scores.len() || top_k == 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let k = top_k.min(relevance.len());
        let mut order = (0..relevance.len()).collect::<Vec<_>>();
        order.sort_by(|&a, &b| {
            scores[b]
                .partial_cmp(&scores[a])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(&b))
        });
        let mut dcg = 0.0;
        let mut hit = 0.0;
        let mut mrr = 0.0;
        for (rank, &idx) in order.iter().take(k).enumerate() {
            if relevance[idx] > 0 {
                hit = 1.0;
                if mrr == 0.0 {
                    mrr = 1.0 / (rank + 1) as f64;
                }
            }
            let gain = (2.0f64).powi(relevance[idx] as i32) - 1.0;
            dcg += gain / ((rank + 2) as f64).log2();
        }
        let mut ideal = relevance.clone();
        ideal.sort_by(|a, b| b.cmp(a));
        let mut idcg = 0.0;
        for (rank, rel) in ideal.iter().take(k).enumerate() {
            let gain = (2.0f64).powi(*rel as i32) - 1.0;
            idcg += gain / ((rank + 2) as f64).log2();
        }
        let ndcg = if idcg == 0.0 { 0.0 } else { dcg / idcg };
        let payload = ml_metrics_json(
            "ranking",
            &[
                ("count", relevance.len().to_string()),
                ("top_k", k.to_string()),
                ("hit_rate_at_k", ml_float_json(hit)),
                ("mrr", ml_float_json(mrr)),
                ("ndcg_at_k", ml_float_json(ndcg)),
            ],
        );
        tensor_result(ctx_ref, alloc_spectra_string(&payload))
    }
}

// ── StatsEmbed ──
//
// Real generative perplexity when per-token log-probs are supplied:
// perplexity = exp(-mean(logp)). Log-probs must be f64 <= 0 (NaN rejected);
// -inf propagates honestly as an infinite perplexity (JSON null downstream).
// Without log-probs no fake number is invented: `perplexity` renders null and
// the old lexical proxy value stays available under its honest name
// `answer_overlap_score` for existing callers.
pub(crate) extern "C" fn std_ml_metrics_generation(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        if ctx.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 && ctx_ref.arg_len != 3 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let Some(output) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(reference) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let output_tokens = ml_token_set(&output);
        let reference_tokens = ml_token_set(&reference);
        let overlap_f1 = ml_f1_overlap(&output, &reference);
        let exact_match = if output.trim() == reference.trim() {
            1.0
        } else {
            0.0
        };
        let answer_overlap_score = if overlap_f1 <= 0.0 {
            f64::INFINITY
        } else {
            (-overlap_f1.ln()).exp()
        };
        let mut perplexity_field = "null".to_string();
        let mut extra_fields: Vec<(&str, String)> = Vec::new();
        if ctx_ref.arg_len == 3 {
            let Some((_, logprobs, _)) = ml_tensor_float_data(args[2] as usize) else {
                return HOST_STATUS_NOT_FOUND;
            };
            let Some((token_count, mean_logprob, perplexity)) =
                ml_perplexity_from_logprobs(&logprobs)
            else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            extra_fields.push(("logprob_tokens", token_count.to_string()));
            extra_fields.push(("logprob_mean", ml_float_json(mean_logprob)));
            perplexity_field = ml_float_json(perplexity);
        }
        let mut fields: Vec<(&str, String)> = vec![
            ("output_tokens", output_tokens.len().to_string()),
            ("reference_tokens", reference_tokens.len().to_string()),
            ("exact_match", ml_float_json(exact_match)),
            ("token_f1", ml_float_json(overlap_f1)),
            ("answer_overlap_score", ml_float_json(answer_overlap_score)),
            ("perplexity", perplexity_field),
        ];
        fields.extend(extra_fields);
        let payload = ml_metrics_json("generation", &fields);
        tensor_result(ctx_ref, alloc_spectra_string(&payload))
    }
}

pub(crate) extern "C" fn std_ml_serving_metrics(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(mut latencies) = ml_tensor_int_data(args[0] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let request_count = args[1];
        let error_count = args[2];
        if latencies.is_empty()
            || request_count <= 0
            || error_count < 0
            || error_count > request_count
        {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        latencies.sort();
        let total_ms = latencies.iter().sum::<i64>().max(0) as f64;
        let avg_ms = total_ms / latencies.len() as f64;
        let p95_index = ((latencies.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
        let p95_ms = latencies[p95_index.min(latencies.len() - 1)] as f64;
        let throughput = if total_ms <= 0.0 {
            request_count as f64
        } else {
            request_count as f64 / (total_ms / 1000.0)
        };
        let payload = ml_metrics_json(
            "serving",
            &[
                ("requests", request_count.to_string()),
                ("errors", error_count.to_string()),
                (
                    "error_rate",
                    ml_float_json(error_count as f64 / request_count as f64),
                ),
                ("latency_avg_ms", ml_float_json(avg_ms)),
                ("latency_p95_ms", ml_float_json(p95_ms)),
                ("throughput_per_second", ml_float_json(throughput)),
            ],
        );
        tensor_result(ctx_ref, alloc_spectra_string(&payload))
    }
}

pub(crate) extern "C" fn std_ml_evaluation_report(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 7) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(name) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(classification) = ml_json_payload_arg(args[2]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(regression) = ml_json_payload_arg(args[3]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(ranking) = ml_json_payload_arg(args[4]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(generation) = ml_json_payload_arg(args[5]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(serving) = ml_json_payload_arg(args[6]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() && std::fs::create_dir_all(parent).is_err() {
                return HOST_STATUS_INTERNAL_ERROR;
            }
        }
        let report = format!(
            "{{\"schema\":\"spectra.ml.evaluation_report.v1\",\"name\":{},\"classification\":{},\"regression\":{},\"ranking\":{},\"generation\":{},\"serving\":{}}}",
            ml_json_string(&name),
            classification,
            regression,
            ranking,
            generation,
            serving
        );
        if std::fs::write(&path, report).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        let human_path = format!("{path}.txt");
        let human = format!(
            "Spectra ML Evaluation Report\nname={}\nclassification={}\nregression={}\nranking={}\ngeneration={}\nserving={}\n",
            name, classification, regression, ranking, generation, serving
        );
        if std::fs::write(&human_path, human).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        tensor_result(ctx_ref, alloc_spectra_string(&path))
    }
}

// ── StatsEmbed ──
/// Real perplexity from per-token log-probs: exp(-mean(logp)).
/// Returns (token_count, mean_logprob, perplexity); None when the slice is
/// empty or any entry is NaN / positive (log-probs are <= 0 by definition).
/// -inf entries propagate honestly: mean = -inf → perplexity = +inf.
pub(crate) fn ml_perplexity_from_logprobs(logprobs: &[f64]) -> Option<(usize, f64, f64)> {
    if logprobs.is_empty() {
        return None;
    }
    let mut sum = 0.0f64;
    for &logprob in logprobs {
        if logprob.is_nan() || logprob > 0.0 {
            return None;
        }
        sum += logprob;
    }
    let mean = sum / logprobs.len() as f64;
    Some((logprobs.len(), mean, (-mean).exp()))
}
