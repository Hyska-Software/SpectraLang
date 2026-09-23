#[cfg(test)]
mod tests {
    use super::*;
    use crate::{initialize, MemoryStats};
    use std::mem;
    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::runtime_test_guard()
    }

    fn manual_stats() -> MemoryStats {
        initialize().memory_stats()
    }

    #[test]
    fn frame_exit_releases_manual_allocations() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        let baseline = manual_stats().manual;

        let frame = spectra_rt_manual_frame_enter();
        let ptr = spectra_rt_manual_alloc(32);
        assert!(!ptr.is_null());

        let after_alloc = manual_stats().manual;
        assert_eq!(after_alloc.allocations, baseline.allocations + 1);
        assert!(after_alloc.bytes >= baseline.bytes);

        spectra_rt_manual_frame_exit(frame);

        let after_exit = manual_stats().manual;
        assert_eq!(after_exit.allocations, baseline.allocations);
        assert_eq!(after_exit.bytes, baseline.bytes);

        spectra_rt_manual_clear();
    }

    #[test]
    fn manual_clear_resets_frames_and_allocations() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        let baseline = manual_stats().manual;

        let _frame_one = spectra_rt_manual_frame_enter();
        let _frame_two = spectra_rt_manual_frame_enter();
        assert!(!spectra_rt_manual_alloc(8).is_null());
        assert!(!spectra_rt_manual_alloc(16).is_null());

        let raised = manual_stats().manual;
        assert!(raised.allocations >= baseline.allocations + 2);
        assert!(raised.bytes >= baseline.bytes);

        spectra_rt_manual_clear();

        let after_clear = manual_stats().manual;
        assert_eq!(after_clear.allocations, baseline.allocations);
        assert_eq!(after_clear.bytes, baseline.bytes);

        let frame = spectra_rt_manual_frame_enter();
        assert!(!spectra_rt_manual_alloc(24).is_null());
        spectra_rt_manual_frame_exit(frame);

        let after_reuse = manual_stats().manual;
        assert_eq!(after_reuse.allocations, baseline.allocations);
        assert_eq!(after_reuse.bytes, baseline.bytes);

        spectra_rt_manual_clear();
    }

    #[test]
    fn string_fast_abi_matches_std_contract() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        // Packed layout: 3 bytes + one NUL terminator byte.
        let raw = spectra_rt_manual_alloc(4);
        assert!(!raw.is_null());
        unsafe {
            *raw.add(0) = b'a';
            *raw.add(1) = b'b';
            *raw.add(2) = b'c';
            *raw.add(3) = 0;
        }

        let ptr = raw as SpectraHostValue;
        assert_eq!(spectra_rt_string_len(ptr), 3);
        assert_eq!(spectra_rt_string_char_at(ptr, 0), b'a' as i64);
        assert_eq!(spectra_rt_string_char_at(ptr, 2), b'c' as i64);
        assert_eq!(spectra_rt_string_char_at(ptr, 3), -1);
        assert_eq!(spectra_rt_string_char_at(ptr, -1), -1);
        assert_eq!(spectra_rt_string_len(0), 0);
        assert_eq!(spectra_rt_string_char_at(0, 0), -1);

        spectra_rt_manual_clear();
    }

    extern "C" fn host_const() -> i64 {
        42
    }

    extern "C" fn host_inc(value: i64) -> i64 {
        value + 1
    }

    extern "C" fn host_context_add(ctx: *mut SpectraHostCallContext) -> i32 {
        if ctx.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        unsafe {
            let ctx_ref = &mut *ctx;
            if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            if ctx_ref.result_len != 1 || ctx_ref.results.is_null() {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            let args = std::slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
            let results = std::slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = args[0] + args[1];
            HOST_STATUS_SUCCESS
        }
    }

    extern "C" fn host_context_fail(_ctx: *mut SpectraHostCallContext) -> i32 {
        HOST_STATUS_INTERNAL_ERROR
    }

    #[test]
    fn host_register_lookup_and_clear() {
        let _lock = test_guard();
        spectra_rt_host_clear();

        let name = b"spectra.test.const";
        let inserted = spectra_rt_host_register(name.as_ptr(), name.len(), host_const as *const ());
        assert!(inserted);

        let ptr = spectra_rt_host_lookup(name.as_ptr(), name.len());
        assert!(!ptr.is_null());
        let func: extern "C" fn() -> i64 = unsafe { mem::transmute(ptr) };
        assert_eq!(func(), 42);

        let replaced = spectra_rt_host_register(name.as_ptr(), name.len(), host_inc as *const ());
        assert!(!replaced);

        let ptr = spectra_rt_host_lookup(name.as_ptr(), name.len());
        let func: extern "C" fn(i64) -> i64 = unsafe { mem::transmute(ptr) };
        assert_eq!(func(41), 42);

        spectra_rt_host_clear();
        assert!(spectra_rt_host_lookup(name.as_ptr(), name.len()).is_null());
    }

    #[test]
    fn host_unregister_removes_entry() {
        let _lock = test_guard();
        spectra_rt_host_clear();

        let name = b"spectra.test.inc";
        spectra_rt_host_register(name.as_ptr(), name.len(), host_inc as *const ());
        assert!(!spectra_rt_host_lookup(name.as_ptr(), name.len()).is_null());

        assert!(spectra_rt_host_unregister(name.as_ptr(), name.len()));
        assert!(spectra_rt_host_lookup(name.as_ptr(), name.len()).is_null());

        assert!(!spectra_rt_host_unregister(name.as_ptr(), name.len()));

        spectra_rt_host_clear();
    }

    #[test]
    fn debug_invariants_cover_host_registry_and_manual_allocations() {
        let _lock = test_guard();
        spectra_rt_host_clear();
        spectra_rt_manual_clear();
        assert!(spectra_rt_debug_invariants_check());

        let frame = spectra_rt_manual_frame_enter();
        let ptr = spectra_rt_manual_alloc(64);
        assert!(!ptr.is_null());
        assert!(spectra_rt_debug_invariants_check());
        spectra_rt_manual_frame_exit(frame);
        assert!(spectra_rt_debug_invariants_check());

        let name = b"spectra.test.context_add";
        assert!(spectra_rt_host_register(
            name.as_ptr(),
            name.len(),
            host_context_add as *const ()
        ));
        assert!(spectra_rt_debug_invariants_check());

        spectra_rt_host_clear();
        spectra_rt_manual_clear();
    }

    #[test]
    fn host_invoke_returns_status_and_writes_results() {
        let _lock = test_guard();
        spectra_rt_host_clear();

        let name = b"spectra.test.context_add";
        assert!(spectra_rt_host_register(
            name.as_ptr(),
            name.len(),
            host_context_add as *const ()
        ));
        let args = [20, 22];
        let mut results = [0];
        let status = spectra_rt_host_invoke(
            name.as_ptr(),
            name.len(),
            args.as_ptr(),
            args.len(),
            results.as_mut_ptr(),
            results.len(),
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(results[0], 42);

        let missing = b"spectra.test.missing";
        let status = spectra_rt_host_invoke(
            missing.as_ptr(),
            missing.len(),
            args.as_ptr(),
            args.len(),
            results.as_mut_ptr(),
            results.len(),
        );
        assert_eq!(status, HOST_STATUS_NOT_FOUND);

        spectra_rt_host_clear();
    }

    #[test]
    fn host_invoke_batch_preserves_order_and_results() {
        let _lock = test_guard();
        spectra_rt_host_clear();

        let name = b"spectra.test.context_add";
        assert!(spectra_rt_host_register(
            name.as_ptr(),
            name.len(),
            host_context_add as *const ()
        ));

        let args_one = [20, 22];
        let args_two = [3, 4];
        let mut result_one = [0];
        let mut result_two = [0];
        let calls = [
            SpectraHostBatchCall {
                name_ptr: name.as_ptr(),
                name_len: name.len(),
                args_ptr: args_one.as_ptr(),
                arg_len: args_one.len(),
                results_ptr: result_one.as_mut_ptr(),
                result_len: result_one.len(),
            },
            SpectraHostBatchCall {
                name_ptr: name.as_ptr(),
                name_len: name.len(),
                args_ptr: args_two.as_ptr(),
                arg_len: args_two.len(),
                results_ptr: result_two.as_mut_ptr(),
                result_len: result_two.len(),
            },
        ];

        assert_eq!(
            spectra_rt_host_invoke_batch(calls.as_ptr(), calls.len()),
            HOST_STATUS_SUCCESS
        );
        assert_eq!(result_one[0], 42);
        assert_eq!(result_two[0], 7);

        spectra_rt_host_clear();
    }

    #[test]
    fn host_invoke_batch_stops_at_first_failure() {
        let _lock = test_guard();
        spectra_rt_host_clear();

        let add_name = b"spectra.test.context_add";
        let fail_name = b"spectra.test.context_fail";
        assert!(spectra_rt_host_register(
            add_name.as_ptr(),
            add_name.len(),
            host_context_add as *const ()
        ));
        assert!(spectra_rt_host_register(
            fail_name.as_ptr(),
            fail_name.len(),
            host_context_fail as *const ()
        ));

        let args = [1, 2];
        let mut first = [0];
        let mut after_failure = [99];
        let calls = [
            SpectraHostBatchCall {
                name_ptr: add_name.as_ptr(),
                name_len: add_name.len(),
                args_ptr: args.as_ptr(),
                arg_len: args.len(),
                results_ptr: first.as_mut_ptr(),
                result_len: first.len(),
            },
            SpectraHostBatchCall {
                name_ptr: fail_name.as_ptr(),
                name_len: fail_name.len(),
                args_ptr: std::ptr::null(),
                arg_len: 0,
                results_ptr: std::ptr::null_mut(),
                result_len: 0,
            },
            SpectraHostBatchCall {
                name_ptr: add_name.as_ptr(),
                name_len: add_name.len(),
                args_ptr: args.as_ptr(),
                arg_len: args.len(),
                results_ptr: after_failure.as_mut_ptr(),
                result_len: after_failure.len(),
            },
        ];

        assert_eq!(
            spectra_rt_host_invoke_batch(calls.as_ptr(), calls.len()),
            HOST_STATUS_INTERNAL_ERROR
        );
        assert_eq!(first[0], 3);
        assert_eq!(after_failure[0], 99);

        spectra_rt_host_clear();
    }

    #[test]
    fn host_invoke_batch_rejects_invalid_descriptor() {
        let _lock = test_guard();
        let calls = [SpectraHostBatchCall {
            name_ptr: std::ptr::null(),
            name_len: 1,
            args_ptr: std::ptr::null(),
            arg_len: 0,
            results_ptr: std::ptr::null_mut(),
            result_len: 0,
        }];

        assert_eq!(
            spectra_rt_host_invoke_batch(calls.as_ptr(), calls.len()),
            HOST_STATUS_INVALID_ARGUMENT
        );
    }

    #[test]
    fn cached_host_invoke_observes_registration_replacement_and_clear() {
        let _lock = test_guard();
        spectra_rt_host_clear();

        let cache = SpectraHostCallCache::new();
        let name = b"spectra.test.cached_add";
        let args = [20, 22];
        let mut results = [0];

        assert_eq!(
            spectra_rt_host_invoke_cached(
                &cache,
                name.as_ptr(),
                name.len(),
                args.as_ptr(),
                args.len(),
                results.as_mut_ptr(),
                results.len(),
            ),
            HOST_STATUS_NOT_FOUND
        );

        assert!(spectra_rt_host_register(
            name.as_ptr(),
            name.len(),
            host_context_add as *const ()
        ));
        assert_eq!(
            spectra_rt_host_invoke_cached(
                &cache,
                name.as_ptr(),
                name.len(),
                args.as_ptr(),
                args.len(),
                results.as_mut_ptr(),
                results.len(),
            ),
            HOST_STATUS_SUCCESS
        );
        assert_eq!(results[0], 42);

        assert!(!spectra_rt_host_register(
            name.as_ptr(),
            name.len(),
            host_context_fail as *const ()
        ));
        assert_eq!(
            spectra_rt_host_invoke_cached(
                &cache,
                name.as_ptr(),
                name.len(),
                args.as_ptr(),
                args.len(),
                results.as_mut_ptr(),
                results.len(),
            ),
            HOST_STATUS_INTERNAL_ERROR
        );

        assert!(spectra_rt_host_unregister(name.as_ptr(), name.len()));
        assert_eq!(
            spectra_rt_host_invoke_cached(
                &cache,
                name.as_ptr(),
                name.len(),
                args.as_ptr(),
                args.len(),
                results.as_mut_ptr(),
                results.len(),
            ),
            HOST_STATUS_NOT_FOUND
        );

        assert!(spectra_rt_host_register(
            name.as_ptr(),
            name.len(),
            host_context_add as *const ()
        ));
        spectra_rt_host_clear();
        assert_eq!(
            spectra_rt_host_invoke_cached(
                &cache,
                name.as_ptr(),
                name.len(),
                args.as_ptr(),
                args.len(),
                results.as_mut_ptr(),
                results.len(),
            ),
            HOST_STATUS_NOT_FOUND
        );
    }

    #[test]
    fn cached_host_invoke_revalidates_after_concurrent_registry_mutation() {
        let _lock = test_guard();
        spectra_rt_host_clear();

        static NAME: &[u8] = b"spectra.test.cached_concurrent";
        let cache = SpectraHostCallCache::new();
        let args = [20, 22];
        let mut result = [0];
        assert!(spectra_rt_host_register(
            NAME.as_ptr(),
            NAME.len(),
            host_context_add as *const ()
        ));
        assert_eq!(
            spectra_rt_host_invoke_cached(
                &cache,
                NAME.as_ptr(),
                NAME.len(),
                args.as_ptr(),
                args.len(),
                result.as_mut_ptr(),
                result.len(),
            ),
            HOST_STATUS_SUCCESS
        );

        let mutation = std::thread::spawn(|| {
            assert!(!spectra_rt_host_register(
                NAME.as_ptr(),
                NAME.len(),
                host_context_fail as *const ()
            ));
        });
        mutation.join().expect("registry mutation thread panicked");

        assert_eq!(
            spectra_rt_host_invoke_cached(
                &cache,
                NAME.as_ptr(),
                NAME.len(),
                args.as_ptr(),
                args.len(),
                result.as_mut_ptr(),
                result.len(),
            ),
            HOST_STATUS_INTERNAL_ERROR
        );

        spectra_rt_host_clear();
    }

    #[test]
    fn cached_host_invoke_batch_preserves_order_and_stops_at_first_error() {
        let _lock = test_guard();
        spectra_rt_host_clear();

        let add_name = b"spectra.test.cached_batch_add";
        let add_cache = SpectraHostCallCache::new();
        let fail_cache = SpectraHostCallCache::new();
        assert!(spectra_rt_host_register(
            add_name.as_ptr(),
            add_name.len(),
            host_context_add as *const ()
        ));
        assert!(spectra_rt_host_register(
            b"spectra.test.cached_batch_fail".as_ptr(),
            b"spectra.test.cached_batch_fail".len(),
            host_context_fail as *const ()
        ));

        let args_one = [20, 22];
        let args_two = [3, 4];
        let mut result_one = [0];
        let mut result_two = [0];
        let calls = [
            SpectraHostCachedBatchCall {
                cache_ptr: &add_cache,
                name_ptr: add_name.as_ptr(),
                name_len: add_name.len(),
                args_ptr: args_one.as_ptr(),
                arg_len: args_one.len(),
                results_ptr: result_one.as_mut_ptr(),
                result_len: result_one.len(),
            },
            SpectraHostCachedBatchCall {
                cache_ptr: &add_cache,
                name_ptr: add_name.as_ptr(),
                name_len: add_name.len(),
                args_ptr: args_two.as_ptr(),
                arg_len: args_two.len(),
                results_ptr: result_two.as_mut_ptr(),
                result_len: result_two.len(),
            },
        ];

        assert_eq!(
            spectra_rt_host_invoke_cached_batch(calls.as_ptr(), calls.len()),
            HOST_STATUS_SUCCESS
        );
        assert_eq!(result_one[0], 42);
        assert_eq!(result_two[0], 7);

        let mut after_failure = [99];
        let failure_name = b"spectra.test.cached_batch_fail";
        let failure_calls = [
            SpectraHostCachedBatchCall {
                cache_ptr: &fail_cache,
                name_ptr: failure_name.as_ptr(),
                name_len: failure_name.len(),
                args_ptr: std::ptr::null(),
                arg_len: 0,
                results_ptr: std::ptr::null_mut(),
                result_len: 0,
            },
            SpectraHostCachedBatchCall {
                cache_ptr: &add_cache,
                name_ptr: add_name.as_ptr(),
                name_len: add_name.len(),
                args_ptr: args_one.as_ptr(),
                arg_len: args_one.len(),
                results_ptr: after_failure.as_mut_ptr(),
                result_len: after_failure.len(),
            },
        ];
        assert_eq!(
            spectra_rt_host_invoke_cached_batch(failure_calls.as_ptr(), failure_calls.len()),
            HOST_STATUS_INTERNAL_ERROR
        );
        assert_eq!(after_failure[0], 99);

        spectra_rt_host_clear();
    }

    #[test]
    fn cached_host_invoke_rejects_invalid_descriptor() {
        let _lock = test_guard();
        let calls = [SpectraHostCachedBatchCall {
            cache_ptr: std::ptr::null(),
            name_ptr: std::ptr::null(),
            name_len: 1,
            args_ptr: std::ptr::null(),
            arg_len: 0,
            results_ptr: std::ptr::null_mut(),
            result_len: 0,
        }];

        assert_eq!(
            spectra_rt_host_invoke_cached_batch(calls.as_ptr(), calls.len()),
            HOST_STATUS_INVALID_ARGUMENT
        );
    }

    /// Installs an evaluator that denies exactly `denied` and allows every
    /// other host name. Callers must hold `test_guard()` and clear it after.
    fn deny_only(denied: &'static str) {
        crate::agent::policy_hook::set_policy_evaluator(move |name, _args| {
            if name == denied {
                crate::agent::policy_hook::PolicyDecision::Deny {
                    reason: format!("capability not granted for {name}"),
                }
            } else {
                crate::agent::policy_hook::PolicyDecision::Allow
            }
        });
    }

    #[test]
    fn policy_denial_covers_uncached_single_invoke() {
        let _lock = test_guard();
        spectra_rt_host_clear();
        crate::agent::policy_hook::clear_policy_evaluator();

        let allowed = b"spectra.test.dispatch_allowed";
        let denied = b"spectra.test.dispatch_denied";
        assert!(spectra_rt_host_register(
            allowed.as_ptr(),
            allowed.len(),
            host_context_add as *const ()
        ));
        assert!(spectra_rt_host_register(
            denied.as_ptr(),
            denied.len(),
            host_context_add as *const ()
        ));

        deny_only("spectra.test.dispatch_denied");

        let args = [20, 22];
        let mut results = [0];
        assert_eq!(
            spectra_rt_host_invoke(
                denied.as_ptr(),
                denied.len(),
                args.as_ptr(),
                args.len(),
                results.as_mut_ptr(),
                results.len(),
            ),
            HOST_STATUS_DENIED
        );
        assert_eq!(results[0], 0, "a denied call must not write a result");

        assert_eq!(
            spectra_rt_host_invoke(
                allowed.as_ptr(),
                allowed.len(),
                args.as_ptr(),
                args.len(),
                results.as_mut_ptr(),
                results.len(),
            ),
            HOST_STATUS_SUCCESS
        );
        assert_eq!(results[0], 42);

        crate::agent::policy_hook::clear_policy_evaluator();
        spectra_rt_host_clear();
    }

    #[test]
    fn policy_denial_covers_cached_single_invoke_on_cache_hits() {
        let _lock = test_guard();
        spectra_rt_host_clear();
        crate::agent::policy_hook::clear_policy_evaluator();

        let denied = b"spectra.test.dispatch_cached_denied";
        assert!(spectra_rt_host_register(
            denied.as_ptr(),
            denied.len(),
            host_context_add as *const ()
        ));
        deny_only("spectra.test.dispatch_cached_denied");

        let cache = SpectraHostCallCache::new();
        let args = [20, 22];
        let mut results = [0];
        assert_eq!(
            spectra_rt_host_invoke_cached(
                &cache,
                denied.as_ptr(),
                denied.len(),
                args.as_ptr(),
                args.len(),
                results.as_mut_ptr(),
                results.len(),
            ),
            HOST_STATUS_DENIED
        );
        assert!(
            !cache
                .function
                .load(std::sync::atomic::Ordering::Acquire)
                .is_null(),
            "the first call must publish the cache slot so the second call is a hit"
        );
        // The second call resolves without consulting the registry, so this
        // exercises policy evaluation on the cache-hit path.
        assert_eq!(
            spectra_rt_host_invoke_cached(
                &cache,
                denied.as_ptr(),
                denied.len(),
                args.as_ptr(),
                args.len(),
                results.as_mut_ptr(),
                results.len(),
            ),
            HOST_STATUS_DENIED
        );
        assert_eq!(results[0], 0);

        crate::agent::policy_hook::clear_policy_evaluator();
        spectra_rt_host_clear();
    }

    #[test]
    fn policy_denial_covers_uncached_batch_per_item() {
        let _lock = test_guard();
        spectra_rt_host_clear();
        crate::agent::policy_hook::clear_policy_evaluator();

        let allowed = b"spectra.test.dispatch_batch_allowed";
        let denied = b"spectra.test.dispatch_batch_denied";
        assert!(spectra_rt_host_register(
            allowed.as_ptr(),
            allowed.len(),
            host_context_add as *const ()
        ));
        assert!(spectra_rt_host_register(
            denied.as_ptr(),
            denied.len(),
            host_context_add as *const ()
        ));
        deny_only("spectra.test.dispatch_batch_denied");

        let args_one = [1, 2];
        let args_two = [3, 4];
        let args_three = [5, 6];
        let mut result_one = [0];
        let mut result_two = [0];
        let mut result_three = [99];
        let calls = [
            SpectraHostBatchCall {
                name_ptr: allowed.as_ptr(),
                name_len: allowed.len(),
                args_ptr: args_one.as_ptr(),
                arg_len: args_one.len(),
                results_ptr: result_one.as_mut_ptr(),
                result_len: result_one.len(),
            },
            SpectraHostBatchCall {
                name_ptr: denied.as_ptr(),
                name_len: denied.len(),
                args_ptr: args_two.as_ptr(),
                arg_len: args_two.len(),
                results_ptr: result_two.as_mut_ptr(),
                result_len: result_two.len(),
            },
            SpectraHostBatchCall {
                name_ptr: allowed.as_ptr(),
                name_len: allowed.len(),
                args_ptr: args_three.as_ptr(),
                arg_len: args_three.len(),
                results_ptr: result_three.as_mut_ptr(),
                result_len: result_three.len(),
            },
        ];

        // The allowed item is evaluated and dispatched; the denied item is
        // evaluated per item and stops the batch before the third descriptor.
        assert_eq!(
            spectra_rt_host_invoke_batch(calls.as_ptr(), calls.len()),
            HOST_STATUS_DENIED
        );
        assert_eq!(result_one[0], 3);
        assert_eq!(result_two[0], 0);
        assert_eq!(result_three[0], 99);

        crate::agent::policy_hook::clear_policy_evaluator();
        spectra_rt_host_clear();
    }

    #[test]
    fn policy_denial_covers_cached_batch_including_hits() {
        let _lock = test_guard();
        spectra_rt_host_clear();
        crate::agent::policy_hook::clear_policy_evaluator();

        let denied = b"spectra.test.dispatch_cached_batch_denied";
        assert!(spectra_rt_host_register(
            denied.as_ptr(),
            denied.len(),
            host_context_add as *const ()
        ));
        deny_only("spectra.test.dispatch_cached_batch_denied");

        let cache = SpectraHostCallCache::new();
        let args = [20, 22];
        let mut results = [0];
        let calls = [SpectraHostCachedBatchCall {
            cache_ptr: &cache,
            name_ptr: denied.as_ptr(),
            name_len: denied.len(),
            args_ptr: args.as_ptr(),
            arg_len: args.len(),
            results_ptr: results.as_mut_ptr(),
            result_len: results.len(),
        }];

        assert_eq!(
            spectra_rt_host_invoke_cached_batch(calls.as_ptr(), calls.len()),
            HOST_STATUS_DENIED
        );
        assert!(
            !cache
                .function
                .load(std::sync::atomic::Ordering::Acquire)
                .is_null(),
            "the first call must publish the cache slot so the second call is a hit"
        );
        assert_eq!(
            spectra_rt_host_invoke_cached_batch(calls.as_ptr(), calls.len()),
            HOST_STATUS_DENIED
        );
        assert_eq!(results[0], 0);

        crate::agent::policy_hook::clear_policy_evaluator();
        spectra_rt_host_clear();
    }

    #[test]
    fn string_fast_abi_uses_allocation_table_bounds() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        // 4-byte allocation but the string ends after one byte: the table
        // bounds the scan while the NUL terminator still ends the string.
        let raw = spectra_rt_manual_alloc(4);
        assert!(!raw.is_null());
        unsafe {
            *raw.add(0) = b'x';
            *raw.add(1) = 0;
            *raw.add(2) = 0x7F;
            *raw.add(3) = 0x7F;
        }
        let ptr = raw as SpectraHostValue;

        assert_eq!(spectra_rt_string_len(ptr), 1);
        assert_eq!(spectra_rt_string_char_at(ptr, 0), b'x' as i64);
        // Terminator respected even though the allocation has spare slots.
        assert_eq!(spectra_rt_string_char_at(ptr, 1), -1);
        // In-range for the allocation but past the terminator.
        assert_eq!(spectra_rt_string_char_at(ptr, 3), -1);
        // Far out of range: rejected without dereferencing.
        assert_eq!(spectra_rt_string_char_at(ptr, 1 << 40), -1);

        spectra_rt_manual_clear();
    }

    #[test]
    fn string_fast_abi_falls_back_to_limited_scan_for_untracked_pointers() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        // Not allocated through spectra_rt_manual_alloc, so unknown to the
        // AllocationTable; the conservative bounded scan must still work.
        let buf = [b'h', b'i', 0, 0];
        let ptr = buf.as_ptr() as SpectraHostValue;

        assert_eq!(spectra_rt_string_len(ptr), 2);
        assert_eq!(spectra_rt_string_char_at(ptr, 0), b'h' as i64);
        assert_eq!(spectra_rt_string_char_at(ptr, 1), b'i' as i64);
        assert_eq!(spectra_rt_string_char_at(ptr, 2), -1);
    }

    #[test]
    fn string_packed_layout_roundtrip_multibyte_utf8() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        // Packed layout: every string is `bytes.len() + 1` bytes, one byte
        // per UTF-8 byte, NUL-terminated. Multibyte characters must survive
        // the round trip byte-exactly.
        for s in ["á", "日", "🎉", "héllo wörld 日本語 🎉"] {
            let bytes = s.as_bytes();
            let raw = spectra_rt_manual_alloc(bytes.len() + 1);
            assert!(!raw.is_null());
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), raw, bytes.len());
                *raw.add(bytes.len()) = 0;
            }
            let ptr = raw as SpectraHostValue;

            assert_eq!(spectra_rt_string_len(ptr), bytes.len() as i64, "{s}");
            for (i, &b) in bytes.iter().enumerate() {
                assert_eq!(spectra_rt_string_char_at(ptr, i as i64), b as i64, "{s}[{i}]");
            }
            assert_eq!(spectra_rt_string_char_at(ptr, bytes.len() as i64), -1);
            assert_eq!(spectra_rt_string_char_at(ptr, 1 << 40), -1);

            spectra_rt_manual_clear();
        }
    }

    #[test]
    fn string_allocation_size_is_packed_bytes_not_slots() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        let baseline = manual_stats().manual;

        // One 100_000-byte payload + 1 terminator byte. Under the previous
        // slot layout this allocation cost (100_000 + 1) * 8 bytes; with
        // packed strings the AllocationTable records exactly 100_001.
        const PAYLOAD_BYTES: usize = 100_000;
        let raw = spectra_rt_manual_alloc(PAYLOAD_BYTES + 1);
        assert!(!raw.is_null());

        let after = manual_stats().manual;
        assert_eq!(after.allocations, baseline.allocations + 1);
        assert_eq!(after.bytes, baseline.bytes + PAYLOAD_BYTES + 1);

        spectra_rt_manual_clear();
    }

    // ── Free quarantine (stale/double-free detection) ────────────────────

    #[test]
    fn double_free_reports_invalid_argument_status() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        let ptr = spectra_rt_manual_alloc(32);
        assert!(!ptr.is_null());

        spectra_rt_manual_free(ptr);
        assert_eq!(spectra_rt_manual_free_last_status(), HOST_STATUS_SUCCESS);

        // Second free of the same pointer: the tombstone is still inside
        // the quarantine window, so this must be a detectable error rather
        // than a silent wrong-free.
        spectra_rt_manual_free(ptr);
        assert_eq!(
            spectra_rt_manual_free_last_status(),
            HOST_STATUS_INVALID_ARGUMENT
        );

        spectra_rt_manual_clear();
    }

    #[test]
    fn free_of_unknown_pointer_reports_invalid_argument_status() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        // A live allocation unknown to the manual table: freeing it must be
        // reported as an error instead of being silently ignored.
        let foreign = Box::new([0u8; 64]);
        let foreign_ptr = foreign.as_ptr() as *mut u8;
        spectra_rt_manual_free(foreign_ptr);
        assert_eq!(
            spectra_rt_manual_free_last_status(),
            HOST_STATUS_INVALID_ARGUMENT
        );

        // Null free remains a legal no-op.
        spectra_rt_manual_free(std::ptr::null_mut());
        assert_eq!(spectra_rt_manual_free_last_status(), HOST_STATUS_SUCCESS);

        spectra_rt_manual_clear();
    }

    #[test]
    fn quarantine_blocks_address_reuse_within_window() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        let first = spectra_rt_manual_alloc(32);
        assert!(!first.is_null());
        spectra_rt_manual_free(first);
        assert!(spectra_rt_manual_quarantine_len() >= 1);

        // While the tombstone is quarantined its heap block stays pinned,
        // so a same-size reallocation cannot legally hand back `first`.
        let second = spectra_rt_manual_alloc(32);
        assert!(!second.is_null());
        assert_ne!(second, first, "quarantined address was reused");

        // Freeing the stale `first` inside the window is detected.
        spectra_rt_manual_free(first);
        assert_eq!(
            spectra_rt_manual_free_last_status(),
            HOST_STATUS_INVALID_ARGUMENT
        );

        // The fresh allocation frees normally.
        spectra_rt_manual_free(second);
        assert_eq!(spectra_rt_manual_free_last_status(), HOST_STATUS_SUCCESS);

        spectra_rt_manual_clear();
    }

    #[test]
    fn quarantine_eviction_restores_legal_reuse_after_full_window() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        // Distinct sizes keep every freed address distinct and FIFO-ordered.
        for index in 0..=QUARANTINE_CAPACITY {
            let size = 32 + index;
            let ptr = spectra_rt_manual_alloc(size);
            assert!(!ptr.is_null());
            spectra_rt_manual_free(ptr);
        }

        // Window full: exactly QUARANTINE_CAPACITY tombstones survive; the
        // oldest (the size-32 block) has been evicted and its memory
        // released for normal reuse.
        assert_eq!(spectra_rt_manual_quarantine_len(), QUARANTINE_CAPACITY);

        // Reuse of the evicted address is legal again. A stale free of it
        // now degrades to the generic unknown-address error path — still
        // detectable, never a silent wrong-free (documented trade-off).
        let reused = spectra_rt_manual_alloc(32);
        assert!(!reused.is_null());
        spectra_rt_manual_free(reused);
        assert_eq!(spectra_rt_manual_free_last_status(), HOST_STATUS_SUCCESS);

        spectra_rt_manual_clear();
        assert_eq!(spectra_rt_manual_quarantine_len(), 0);
    }

    /// An image-owned literal is readable as a string without being owned: the
    /// runtime sees its length, and freeing it does not release the image's
    /// bytes (nor is it reported as a leak).
    #[test]
    fn registered_literals_are_readable_but_never_freed() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        let bytes: Vec<u8> = b"literal-key\0".to_vec();
        let ptr = bytes.as_ptr() as i64;
        let len = bytes.len() as i64;
        spectra_rt_register_literal(ptr, len);

        assert_eq!(manual_allocation_size(ptr), Some(bytes.len()));
        assert_eq!(
            unsafe { crate::stdlib::try_read_packed_string(ptr) },
            Some("literal-key".to_string())
        );

        // Freeing an image-owned literal is refused: the runtime does not own it.
        spectra_rt_manual_free(ptr as *mut u8);
        assert_ne!(spectra_rt_manual_free_last_status(), HOST_STATUS_SUCCESS);
        // …and it is still readable afterwards.
        assert_eq!(manual_allocation_size(ptr), Some(bytes.len()));

        spectra_rt_manual_clear();
    }

    /// A value a container stores must outlive the frame that produced it: the
    /// container is runtime-owned and can be read long after the pushing frame
    /// exited, so the store escapes the value first.
    #[test]
    fn stored_values_escape_their_producing_frame() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        let frame = spectra_rt_manual_frame_enter();
        let stored = spectra_rt_manual_alloc(32);
        let dropped = spectra_rt_manual_alloc(32);
        assert!(!stored.is_null() && !dropped.is_null());

        // What a list or map store does with the value it keeps.
        crate::ffi::escape_stored_value(stored as i64);

        spectra_rt_manual_frame_exit(frame);

        // The stored block is still a live allocation: freeing it succeeds.
        spectra_rt_manual_free(stored);
        assert_eq!(spectra_rt_manual_free_last_status(), HOST_STATUS_SUCCESS);

        // The sibling the container never took was released by the frame exit,
        // so a second free is reported as invalid instead of succeeding.
        spectra_rt_manual_free(dropped);
        assert_ne!(spectra_rt_manual_free_last_status(), HOST_STATUS_SUCCESS);

        spectra_rt_manual_clear();
    }

    #[test]
    fn stored_values_escape_an_outer_frame_when_written_from_a_nested_frame() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        let outer = spectra_rt_manual_frame_enter();
        let stored = spectra_rt_manual_alloc(32);
        assert!(!stored.is_null());
        let inner = spectra_rt_manual_frame_enter();

        crate::ffi::escape_stored_value(stored as i64);

        spectra_rt_manual_frame_exit(inner);
        spectra_rt_manual_frame_exit(outer);
        assert_eq!(manual_allocation_size(stored as i64), Some(32));
        spectra_rt_manual_free(stored);
        assert_eq!(spectra_rt_manual_free_last_status(), HOST_STATUS_SUCCESS);
        spectra_rt_manual_clear();
    }

    #[test]
    fn alloc_free_pressure_maintains_invariants_and_stats() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        let baseline = manual_stats().manual;

        // Churn well past the quarantine capacity with mixed sizes and an
        // escape in the middle; every step must leave the table consistent.
        for round in 0..4 * QUARANTINE_CAPACITY {
            let frame = spectra_rt_manual_frame_enter();
            let a = spectra_rt_manual_alloc(16 + round % 7);
            assert!(!a.is_null());
            let b = spectra_rt_manual_alloc(48 + round % 5);
            assert!(!b.is_null());
            spectra_rt_manual_escape(a, frame);
            spectra_rt_manual_frame_exit(frame);

            if round % 16 == 0 {
                assert!(spectra_rt_debug_invariants_check());
            }
        }
        assert!(spectra_rt_debug_invariants_check());
        assert!(spectra_rt_manual_quarantine_len() <= QUARANTINE_CAPACITY);

        // Quarantined buffers release their statistics at free time, so the
        // only live accounting left is the escaped allocation (one per
        // round).
        let after = manual_stats().manual;
        assert_eq!(
            after.allocations,
            baseline.allocations + 4 * QUARANTINE_CAPACITY
        );

        spectra_rt_manual_clear();
        assert!(spectra_rt_debug_invariants_check());
    }
}

// ── Fast-path symbol retention ────────────────────────────────────────────────
//
// The `spectra_rt_*_fast` functions are not called from any Rust code: they
// are resolved at runtime by the JIT's symbol resolver (cranelift-jit calls
// `GetProcAddress` / `dlsym` against the running process image). Because
// nothing in `spectra-cli` references them, the linker treats them as dead
// code and strips them, which then causes runtime panics such as
// `can't resolve symbol spectra_rt_channel_new_fast`.
//
// Two complementary mechanisms keep every fast-path symbol alive in the
// final binary across all targets (including MSVC, which strips
// unreferenced functions even when other functions in the same TU call
// them through a `#[no_mangle]` re-export):
//
//  1. Each `pub extern "C" fn spectra_rt_*_fast` is also marked
//     `#[inline(never)]`, so the compiler emits a real function body that
//     can be addressed by the JIT.
//  2. `pub fn keep_fast_symbols` calls each fast function with safe dummy
//     inputs and discards the results. The call is invoked once at startup
//     from `crate::stdlib::register`, which itself is called from
//     `spectra_runtime::register` in `spectra-cli`, so
//     every fast-path symbol survives dead-code elimination.
//
// To keep them across all targets (including MSVC, where `#[used]` on a
// `fn` itself is not supported and `#[used]` statics do not pull in the
// functions they reference), `pub fn keep_fast_symbols` calls each one
// with safe dummy inputs and discards the results. The call is invoked
// once at startup from `crate::stdlib::register`, which itself is called
// from `spectra_runtime::register` in `spectra-cli`, so
// every fast-path symbol survives dead-code elimination.
//
// The functions are designed to be side-effect-safe when called with valid
// registry state: `*_new_*` allocates a fresh handle (cheap), `*_get_*` /
// `*_contains_*` / `*_len_*` are read-only, and the rest are no-ops on
// invalid handles (they return NOT_FOUND / 0). Because the handles they
// return are immediately dropped without being used again, no observable
// state changes.

// ============================================================================
// APPEND-ONLY (Frame0Budget): frame-0 escape budget tests. Tests for the
// frame-0 budget belong here at the end of this shared file; do not modify
// earlier sections.
// ============================================================================

#[cfg(test)]
mod frame0_budget_tests {
    use super::*;
    use std::process::Command;

    const CHILD_TEST: &str = "frame0_budget_child_escapes_past_budget";

    #[test]
    fn budget_allows_bytes_within_limit() {
        assert!(frame0_budget_check(0, 512).is_ok());
        assert!(frame0_budget_check(512 * 1024 * 1024, 512).is_ok());
    }

    #[test]
    fn budget_breaches_strictly_past_limit_with_diagnostic() {
        let err = frame0_budget_check(512 * 1024 * 1024 + 1, 512).unwrap_err();
        assert!(err.contains("frame-0 budget exceeded"), "{err}");
        assert!(err.contains("escaped 513 MB > 512 MB"), "{err}");
        assert!(
            err.contains("raise SPECTRA_FRAME0_BUDGET_MB if intentional"),
            "{err}"
        );
    }

    #[test]
    fn budget_zero_disables_ceiling() {
        assert!(frame0_budget_check(usize::MAX, 0).is_ok());
    }

    #[test]
    fn budget_parse_env_variants() {
        assert_eq!(parse_frame0_budget_mb(None), 512);
        assert_eq!(parse_frame0_budget_mb(Some("garbage")), 512);
        assert_eq!(parse_frame0_budget_mb(Some("0")), 0);
        assert_eq!(parse_frame0_budget_mb(Some(" 7 ")), 7);
    }

    /// Only acts when launched as a child process by
    /// `frame0_budget_exceeded_aborts_process_with_diagnostic` below: escapes
    /// past a 1 MiB budget and never returns — the runtime aborts via
    /// `spectra_rt_panic` with exit code 101 partway through the loop.
    #[test]
    fn frame0_budget_child_escapes_past_budget() {
        if std::env::var("SPECTRA_FRAME0_BUDGET_CHILD").as_deref() != Ok("1") {
            return;
        }
        let _lock = crate::runtime_test_guard();
        // 128 × 16 KiB = 2 MiB of escapes against a 1 MiB budget.
        for _ in 0..128 {
            let frame = spectra_rt_manual_frame_enter();
            let ptr = spectra_rt_manual_alloc(16 * 1024);
            assert!(!ptr.is_null());
            spectra_rt_manual_escape(ptr, frame);
            spectra_rt_manual_frame_exit(frame);
        }
        panic!("frame-0 budget should have aborted this child process");
    }

    #[test]
    fn frame0_budget_exceeded_aborts_process_with_diagnostic() {
        let exe = std::env::current_exe().expect("current test binary path");
        let output = Command::new(exe)
            .arg(CHILD_TEST)
            .env("SPECTRA_FRAME0_BUDGET_MB", "1")
            .env("SPECTRA_FRAME0_BUDGET_CHILD", "1")
            .output()
            .expect("spawn child test process");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(crate::panic::SPECTRA_RUNTIME_PANIC_EXIT_CODE),
            "child stderr: {stderr}"
        );
        assert!(stderr.contains("frame-0 budget exceeded"), "stderr: {stderr}");
        // 65 × 16 KiB crosses the 1 MiB ceiling; ceil-rounding reports 2 MB.
        assert!(stderr.contains("escaped 2 MB > 1 MB"), "stderr: {stderr}");
        assert!(stderr.contains("SPECTRA_FRAME0_BUDGET_MB"), "stderr: {stderr}");
    }
}

// ============================================================================
// Thread-safe manual frames, container-registry resets, string-literal
// statuses, and closure-invocation hardening tests. Appended at the end of
// this shared file (mirroring the Frame0Budget convention: earlier sections
// are not modified).
// ============================================================================

#[cfg(test)]
mod thread_safe_frame_tests {
    use super::*;

    /// Thread B's `frame_exit` must never drain frames owned by thread A:
    /// doing so quarantines A's *live* allocations, which becomes a
    /// use-after-free as soon as the quarantine evicts the tombstones.
    #[test]
    fn frame_exit_only_frees_frames_owned_by_the_calling_thread() {
        let _lock = crate::runtime_test_guard();
        spectra_rt_manual_clear();

        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();

        let thread_a = std::thread::spawn(move || {
            let frame = spectra_rt_manual_frame_enter();
            let ptr = spectra_rt_manual_alloc(32);
            assert!(!ptr.is_null());
            unsafe { std::ptr::write_bytes(ptr, 0xAB, 32) };
            ready_tx
                .send((frame, ptr as usize))
                .expect("hand frame state to main thread");
            // Keep the frame open until the main thread (playing "thread B")
            // has run its own enter/exit sequence plus a bogus exit with A's id.
            release_rx.recv().expect("wait for release");

            // B's bogus exit must not have freed this buffer.
            let bytes = unsafe { std::slice::from_raw_parts(ptr, 32) };
            assert!(
                bytes.iter().all(|byte| *byte == 0xAB),
                "thread B's frame_exit freed thread A's live buffer"
            );

            // Exiting its OWN frame frees it normally.
            spectra_rt_manual_frame_exit(frame);
            assert_eq!(
                spectra_rt_manual_frame_exit_last_status(),
                HOST_STATUS_SUCCESS
            );
        });

        let (a_frame, a_ptr) = ready_rx.recv().expect("thread A state");

        // This thread opens its own frame and allocates inside it.
        let b_frame = spectra_rt_manual_frame_enter();
        let b_ptr = spectra_rt_manual_alloc(32);
        assert!(!b_ptr.is_null());

        // Exits with A's frame id: not on THIS thread's stack → no-op.
        spectra_rt_manual_frame_exit(a_frame);
        assert_eq!(
            spectra_rt_manual_frame_exit_last_status(),
            HOST_STATUS_INVALID_ARGUMENT,
            "a frame owned by another thread must be reported, not drained"
        );
        // A's buffer is still live and readable from this thread as well.
        let bytes = unsafe { std::slice::from_raw_parts(a_ptr as *const u8, 32) };
        assert!(
            bytes.iter().all(|byte| *byte == 0xAB),
            "bogus cross-thread frame_exit corrupted A's buffer"
        );

        // This thread's own frame exits normally and frees its allocation.
        spectra_rt_manual_frame_exit(b_frame);
        assert_eq!(
            spectra_rt_manual_frame_exit_last_status(),
            HOST_STATUS_SUCCESS
        );
        spectra_rt_manual_free(b_ptr);
        assert_eq!(
            spectra_rt_manual_free_last_status(),
            HOST_STATUS_INVALID_ARGUMENT,
            "b_ptr must already be quarantined by its own frame exit"
        );

        release_tx.send(()).expect("release thread A");
        thread_a.join().expect("thread A panicked");

        // A's buffer was freed by A's own exit → quarantined → stale free
        // is detected instead of silently succeeding.
        spectra_rt_manual_free(a_ptr as *mut u8);
        assert_eq!(
            spectra_rt_manual_free_last_status(),
            HOST_STATUS_INVALID_ARGUMENT,
            "thread A's buffer must have been freed by A's own frame_exit"
        );

        spectra_rt_manual_clear();
    }

    /// Unknown, stale, cross-thread, and base-frame ids must free NOTHING
    /// (the pre-fix `pop_frame` drained every non-base frame when the id was
    /// not found — killing unrelated live frames).
    #[test]
    fn frame_exit_with_unknown_or_stale_id_frees_nothing() {
        let _lock = crate::runtime_test_guard();
        spectra_rt_manual_clear();

        let outer = spectra_rt_manual_frame_enter();
        let survivor = spectra_rt_manual_alloc(32);
        assert!(!survivor.is_null());
        let inner = spectra_rt_manual_frame_enter();
        let inner_ptr = spectra_rt_manual_alloc(32);
        assert!(!inner_ptr.is_null());

        // Normal exit of this thread's own inner frame.
        spectra_rt_manual_frame_exit(inner);
        assert_eq!(
            spectra_rt_manual_frame_exit_last_status(),
            HOST_STATUS_SUCCESS
        );

        // Unknown id (never existed) → frees nothing.
        spectra_rt_manual_frame_exit(inner + 100_000);
        assert_eq!(
            spectra_rt_manual_frame_exit_last_status(),
            HOST_STATUS_INVALID_ARGUMENT
        );
        // Stale id (already popped) → frees nothing.
        spectra_rt_manual_frame_exit(inner);
        assert_eq!(
            spectra_rt_manual_frame_exit_last_status(),
            HOST_STATUS_INVALID_ARGUMENT
        );
        // The base frame is never exitable.
        spectra_rt_manual_frame_exit(0);
        assert_eq!(
            spectra_rt_manual_frame_exit_last_status(),
            HOST_STATUS_INVALID_ARGUMENT
        );

        // The outer frame's allocation survived every bogus exit above.
        spectra_rt_manual_free(survivor);
        assert_eq!(
            spectra_rt_manual_free_last_status(),
            HOST_STATUS_SUCCESS,
            "a bogus frame_exit must not drain other live frames"
        );

        spectra_rt_manual_frame_exit(outer);
        assert_eq!(
            spectra_rt_manual_frame_exit_last_status(),
            HOST_STATUS_SUCCESS
        );
        spectra_rt_manual_clear();
    }
}

#[cfg(test)]
mod manual_clear_registry_tests {
    use super::*;

    /// `spectra_rt_manual_clear` (called by the CLI after every JIT run)
    /// frees the manual heap; the container registries whose entries are raw
    /// pointers into that heap must be reset with it, otherwise a REPL or
    /// package-test loop hands the next program dangling handles.
    #[test]
    fn manual_clear_resets_container_registries_holding_heap_pointers() {
        let _lock = crate::runtime_test_guard();
        spectra_rt_manual_clear();

        // A list and a map storing a pointer into the manual heap...
        let list = spectra_rt_list_new_fast();
        assert!(list > 0, "list handle must be allocated");
        let payload = spectra_rt_manual_alloc(32);
        assert!(!payload.is_null());
        assert_eq!(
            spectra_rt_list_push_fast(list, payload as SpectraHostValue),
            HOST_STATUS_SUCCESS
        );

        let map = spectra_rt_map_new_fast();
        assert!(map > 0, "map handle must be allocated");
        assert_eq!(
            spectra_rt_map_set_fast(map, payload as SpectraHostValue, 1),
            HOST_STATUS_SUCCESS
        );

        // …a stack, a string builder, and an iterator snapshotting the same
        // pointers.
        let stack = spectra_rt_stack_new_fast();
        assert!(stack > 0);
        assert_eq!(
            spectra_rt_stack_push_fast(stack, payload as SpectraHostValue),
            HOST_STATUS_SUCCESS
        );
        let builder = spectra_rt_builder_new(64);
        assert!(builder > 0);

        spectra_rt_manual_clear();

        // Every handle must be gone: its storage pointed at freed heap.
        assert_ne!(
            crate::stdlib::with_list_registry(|registry| registry.snapshot(list as usize).is_ok()),
            true,
            "list handle must not survive manual_clear"
        );
        // The map fast-path reports NOT_FOUND for a vanished handle — which
        // also proves no new entry can be written into freed backing store.
        assert_eq!(
            spectra_rt_map_set_fast(map, 1, 1),
            HOST_STATUS_NOT_FOUND,
            "map handle must not survive manual_clear"
        );
        assert_eq!(
            spectra_rt_stack_len_fast(stack),
            0,
            "stack handle must not survive manual_clear"
        );
        assert_eq!(
            spectra_rt_builder_len(builder),
            0,
            "string builder handle must not survive manual_clear"
        );

        spectra_rt_manual_clear();
    }
}

#[cfg(test)]
mod string_len_status_tests {
    use super::*;

    /// An unterminated buffer must surface as an error status (`-1`), not as
    /// the length `0` which conflated "empty" with "not a Spectra string".
    #[test]
    fn string_len_reports_unterminated_pointers_as_an_error() {
        let _lock = crate::runtime_test_guard();
        spectra_rt_manual_clear();

        // Tracked allocation with no NUL anywhere inside → error status.
        let raw = spectra_rt_manual_alloc(4);
        assert!(!raw.is_null());
        unsafe { std::ptr::write_bytes(raw, b'A', 4) };
        assert_eq!(
            spectra_rt_string_len(raw as SpectraHostValue),
            -1,
            "an unterminated buffer must report an error, not 0/empty"
        );

        // The same buffer once terminated reports its length again, and the
        // null-pointer contract (empty string → 0) is unchanged.
        unsafe { *raw.add(3) = 0 };
        assert_eq!(spectra_rt_string_len(raw as SpectraHostValue), 3);
        assert_eq!(spectra_rt_string_len(0), 0);
        assert_eq!(spectra_rt_string_char_at(raw as SpectraHostValue, 3), -1);

        spectra_rt_manual_clear();
    }
}

#[cfg(test)]
mod invoke_closure_hardening_tests {
    use super::*;

    extern "C" fn invoke_test_code(_env: i64, arg: i64) -> i64 {
        arg + 1
    }

    /// `spectra_rt_invoke_closure` must validate the closure object and the
    /// code pointer before transmuting anything: garbage objects and code
    /// outside every registered executable range are rejected WITHOUT being
    /// called.
    #[test]
    fn invoke_closure_validates_closure_object_and_code_pointer() {
        let _lock = crate::runtime_test_guard();
        let mut out = 0i64;
        let no_args: [i64; 0] = [];

        // Null is invalid (unchanged).
        assert_eq!(
            unsafe { spectra_rt_invoke_closure(0, no_args.as_ptr(), 0, &mut out) },
            HOST_STATUS_INVALID_ARGUMENT
        );
        // Garbage closure object (address 0x8 is never committed readable).
        assert_eq!(
            unsafe { spectra_rt_invoke_closure(0x8, no_args.as_ptr(), 0, &mut out) },
            HOST_STATUS_INVALID_ARGUMENT,
            "an unmapped closure object must be rejected before dereference"
        );

        // Tracked closure object whose slot 0 holds a non-executable address:
        // rejected WITHOUT calling (the out slot must stay untouched).
        let bogus = spectra_rt_manual_alloc(16);
        assert!(!bogus.is_null());
        unsafe { (bogus as *mut i64).write_unaligned(0x10) };
        out = 0x7777;
        assert_eq!(
            unsafe { spectra_rt_invoke_closure(bogus as i64, no_args.as_ptr(), 0, &mut out) },
            HOST_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(out, 0x7777, "a rejected closure must never run");

        // Tracked object smaller than the code slot → rejected before read.
        let tiny = spectra_rt_manual_alloc(4);
        assert!(!tiny.is_null());
        assert_eq!(
            unsafe { spectra_rt_invoke_closure(tiny as i64, no_args.as_ptr(), 0, &mut out) },
            HOST_STATUS_INVALID_ARGUMENT,
            "a closure object smaller than slot 0 must be rejected"
        );

        // Code-range registry API rejects degenerate ranges.
        assert!(!spectra_rt_register_code_range(0, 0));
        assert!(!spectra_rt_register_code_range(1, 0));
        let range_base = invoke_test_code as *const () as usize as i64;
        assert!(spectra_rt_register_code_range(range_base, 4096));

        // Valid closure: tracked object, code inside an executable range
        // (the test binary image, discovered by the first-use region scan).
        let good = spectra_rt_manual_alloc(16);
        assert!(!good.is_null());
        unsafe {
            (good as *mut i64).write_unaligned(invoke_test_code as *const () as usize as i64)
        };
        let args = [41i64];
        assert_eq!(
            unsafe { spectra_rt_invoke_closure(good as i64, args.as_ptr(), 1, &mut out) },
            HOST_STATUS_SUCCESS,
            "a well-formed closure must still invoke"
        );
        assert_eq!(out, 42);

        spectra_rt_manual_clear();
    }
}
