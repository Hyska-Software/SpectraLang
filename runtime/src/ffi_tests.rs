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
        let raw = spectra_rt_manual_alloc(4) as *mut u8;
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

    #[test]
    fn string_fast_abi_uses_allocation_table_bounds() {
        let _lock = test_guard();
        spectra_rt_manual_clear();

        // 4-byte allocation but the string ends after one byte: the table
        // bounds the scan while the NUL terminator still ends the string.
        let raw = spectra_rt_manual_alloc(4) as *mut u8;
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
            let raw = spectra_rt_manual_alloc(bytes.len() + 1) as *mut u8;
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
        let mut evicted_addr = 0usize;
        for index in 0..=QUARANTINE_CAPACITY {
            let size = 32 + index;
            let ptr = spectra_rt_manual_alloc(size);
            assert!(!ptr.is_null());
            if index == 0 {
                evicted_addr = ptr as usize;
            }
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
//     `spectra_runtime::register_standard_library` in `spectra-cli`, so
//     every fast-path symbol survives dead-code elimination.
//
// To keep them across all targets (including MSVC, where `#[used]` on a
// `fn` itself is not supported and `#[used]` statics do not pull in the
// functions they reference), `pub fn keep_fast_symbols` calls each one
// with safe dummy inputs and discards the results. The call is invoked
// once at startup from `crate::stdlib::register`, which itself is called
// from `spectra_runtime::register_standard_library` in `spectra-cli`, so
// every fast-path symbol survives dead-code elimination.
//
// The functions are designed to be side-effect-safe when called with valid
// registry state: `*_new_*` allocates a fresh handle (cheap), `*_get_*` /
// `*_contains_*` / `*_len_*` are read-only, and the rest are no-ops on
// invalid handles (they return NOT_FOUND / 0). Because the handles they
// return are immediately dropped without being used again, no observable
// state changes.
