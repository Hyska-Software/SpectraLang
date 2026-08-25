#[doc(hidden)]
#[inline(never)]
pub fn keep_fast_symbols() {
    // Concurrent: spawn a task we never join, then drop the channel we open.
    let task = spectra_rt_concurrent_spawn_fast(0);
    let _ = spectra_rt_concurrent_join_fast(task);
    let spawn_fn_task = spectra_rt_concurrent_spawn_fn_fast(0, 0);
    let _ = spectra_rt_concurrent_join_fast(spawn_fn_task);
    let channel = spectra_rt_channel_new_fast();
    let _ = spectra_rt_channel_len_fast(channel);
    let _ = spectra_rt_channel_recv_fast(channel);
    let _ = spectra_rt_channel_send_fast(channel, 0);
    let _ = spectra_rt_channel_close_fast(channel);

    // Map: create a map, write / read / check, then free.
    let m = spectra_rt_map_new_fast();
    let _ = spectra_rt_map_set_fast(m, 0, 0);
    let _ = spectra_rt_map_get_fast(m, 0);
    let _ = spectra_rt_map_contains_fast(m, 0);
    let _ = spectra_rt_map_remove_fast(m, 0);
    let _ = spectra_rt_map_len_fast(m);
    spectra_rt_map_clear_fast(m);
    spectra_rt_map_free_fast(m);

    // Tensor / ML: exercise the full pipeline with a 1-element tensor.
    let t = spectra_rt_tensor_full_f_fast(1, 0.0);
    let _ = spectra_rt_ml_linear_fast(t, t, t);
    let loss = spectra_rt_ml_mse_loss_fast(t, t);
    let _ = spectra_rt_tensor_backward_fast(loss);
    let _ = spectra_rt_ml_sgd_step_fast(t, 0.0);

    // String fast-path: handle 0 is the no-op sentinel (returns 0).
    let _ = spectra_rt_string_len_fast(0);
    let _ = spectra_rt_string_char_at_fast(0, 0);

    // Generic dispatch symbols are also resolved by generated JIT/AOT code;
    // reference them here so release linkers keep both legacy and cache-aware
    // internal entry points in the executable image.
    let _ = spectra_rt_host_invoke(ptr::null(), 0, ptr::null(), 0, ptr::null_mut(), 0);
    let _ = spectra_rt_host_invoke_batch(ptr::null(), 0);
    let _ = spectra_rt_host_invoke_cached(
        ptr::null(),
        ptr::null(),
        0,
        ptr::null(),
        0,
        ptr::null_mut(),
        0,
    );
    let _ = spectra_rt_host_invoke_cached_batch(ptr::null(), 0);
}
