"""Validates the examples/langtest language-testing algorithm suite.

Runs `check` once over the directory and `run` per file. Exits non-zero
on the first failure so it can be used as a gate.
"""

import argparse
import subprocess
import sys
from pathlib import Path

FILES = [
    "01_dfa_lexer.spectra",
    "02_ll1_table_parser.spectra",
    "03_pratt_precedence.spectra",
    "04_hm_unification.spectra",
    "05_dataflow_liveness.spectra",
    "06_fuzz_differential.spectra",
    "07_ddmin_reduction.spectra",
    "08_nfa_thompson.spectra",
    "09_cyk_parser.spectra",
    "10_lr0_closure.spectra",
    "11_reaching_definitions.spectra",
    "12_mutation_testing.spectra",
    "13_quickcheck_properties.spectra",
    "14_interval_abstract.spectra",
    "15_earley_parser.spectra",
    "16_slr_table_parser.spectra",
    "17_constprop_lattice.spectra",
    "18_dominators.spectra",
    "19_levenshtein_diff.spectra",
    "20_concolic_paths.spectra",
    "21_grammar_fuzzer.spectra",
    "22_dpll_sat.spectra",
    "23_tarjan_scc.spectra",
    "24_kmp_search.spectra",
    "25_graph_coloring.spectra",
    "26_mark_sweep.spectra",
    "27_packrat_memo.spectra",
    "28_topo_sort.spectra",
    "29_shunting_yard.spectra",
    "30_first_follow.spectra",
    "31_peephole_opt.spectra",
    "32_stlc_bidir.spectra",
    "33_json_parser.spectra",
    "34_domfrontiers.spectra",
    "35_brzozowski.spectra",
    "36_bmh_search.spectra",
    "37_astar_grid.spectra",
    "38_gvn_numbering.spectra",
    "39_lru_cache.spectra",
    "40_error_recovery.spectra",
    "41_scope_resolution.spectra",
    "42_taint_analysis.spectra",
    "43_aho_corasick.spectra",
    "44_dfa_minimization.spectra",
    "45_model_checking.spectra",
    "46_program_slice.spectra",
    "47_short_circuit.spectra",
    "48_escape_analysis.spectra",
    "49_inline_cost.spectra",
    "50_corpus_minimization.spectra",
    "51_string_interning.spectra",
    "52_burs_tiling.spectra",
    "53_abcd_elimination.spectra",
    "54_glob_matching.spectra",
    "55_left_recursion_elim.spectra",
    "56_callrank_pagerank.spectra",
    "57_minimax_ab.spectra",
    "58_rope_buffer.spectra",
    "59_semver_match.spectra",
    "60_mvs_resolve.spectra",
    "61_http_router.spectra",
    "62_token_bucket.spectra",
    "63_circuit_breaker.spectra",
    "64_sql_predicates.spectra",
    "65_btree_ops.spectra",
    "66_wal_redo.spectra",
    "67_hash_join.spectra",
    "68_histogram_buckets.spectra",
    "69_merkle_proof.spectra",
    "70_rbac_eval.spectra",
    "71_consistent_hashing.spectra",
    "72_wrr_scheduler.spectra",
    "73_deadlock_detect.spectra",
    "74_bankers_safety.spectra",
    "75_ws_frames.spectra",
    "76_quic_varint.spectra",
    "77_url_parse.spectra",
    "78_timer_heap.spectra",
    "79_work_stealing_deque.spectra",
    "80_lsm_tree.spectra",
    "81_jwt_claims.spectra",
    "82_huffman_coding.spectra",
    "83_linear_scan_regalloc.spectra",
    "84_ssa_destruction.spectra",
    "85_vtable_layout.spectra",
    "86_csv_parser.spectra",
    "87_sql_pipeline.spectra",
    "88_bplus_range.spectra",
    "89_mvcc_visibility.spectra",
    "90_adler32_checksum.spectra",
    "91_sched_list.spectra",
    "92_egraph_rewrites.spectra",
    "93_datalog_points_to.spectra",
    "94_twosat_scc.spectra",
    "95_knn_classifier.spectra",
    "96_fft_butterfly.spectra",
    "97_soundex_search.spectra",
    "98_vlq_sourcemap.spectra",
]


def run(binary: str, args: list[str], timeout: int = 60) -> subprocess.CompletedProcess:
    return subprocess.run(
        [binary, *args],
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    cli = parser.parse_args()

    root = Path(__file__).resolve().parent.parent
    suite = root / "examples" / "langtest"

    missing = [f for f in FILES if not (suite / f).exists()]
    if missing:
        print(f"FAIL missing files: {missing}")
        return 1

    check = run(cli.binary, ["check", str(suite)])
    if check.returncode != 0:
        print("FAIL check examples/langtest")
        print(check.stdout[-2000:])
        print(check.stderr[-2000:])
        return 1
    print("PASS check examples/langtest")

    failed = 0
    for name in FILES:
        proc = run(cli.binary, ["run", str(suite / name)])
        ok = proc.returncode == 0
        print(f"{'PASS' if ok else 'FAIL'} run {name} (exit={proc.returncode})")
        if not ok:
            print(proc.stdout[-2000:])
            print(proc.stderr[-2000:])
            failed += 1

    # R527: unit-call branch tails once poisoned merge phis at -O0/-O1
    # ("Value N not found during backend codegen"). Re-run everything
    # unoptimized so the gate stays sensitive to that bug class.
    for name in FILES:
        proc = run(cli.binary, ["run", "-O0", str(suite / name)])
        ok = proc.returncode == 0
        print(f"{'PASS' if ok else 'FAIL'} run -O0 {name} (exit={proc.returncode})")
        if not ok:
            print(proc.stdout[-2000:])
            print(proc.stderr[-2000:])
            failed += 1

    fmt = run(cli.binary, ["fmt", "--check", str(suite)])
    print(f"{'PASS' if fmt.returncode == 0 else 'FAIL'} fmt --check examples/langtest")
    if fmt.returncode != 0:
        failed += 1

    print(f"langtest: {len(FILES) - failed}/{len(FILES)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
