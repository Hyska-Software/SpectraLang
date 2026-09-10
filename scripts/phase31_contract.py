"""Shared Phase 31 benchmark contract.

Keep runner, validator, tests, and documentation aligned on one scenario set.
"""

from __future__ import annotations

PHASE31_SCHEMA = "spectra.phase31.bench.v1"
WARMUP_RUNS = 3
TIMED_RUNS = 20
MAX_STDDEV_PCT = 10.0
DEFAULT_MAX_DRIFT_PCT = 15.0
ASYNC_ECHO_REFERENCE_RUNTIME = "go"
# Accepted by the user for the focused R-3133 release measurement.
ASYNC_ECHO_ACCEPTED_MAX_GAP_TO_GO = 1.202162
ASYNC_ECHO_MAX_REFERENCE_GAP_PCT = (ASYNC_ECHO_ACCEPTED_MAX_GAP_TO_GO - 1.0) * 100.0
ASYNC_ECHO_CONTRACT = "fanout_fanin_real_concurrency.v2"
ASYNC_ECHO_TASKS_PER_ITERATION = 10
ASYNC_ECHO_ITERATIONS = 10_000
ASYNC_ECHO_EXPECTED_RESULT = 550_000
OFFICIAL_INDEPENDENT_RUNS = 5
MAX_CONFIRMATION_RUNS = 2

LANGUAGES = ("spectra", "go", "rust")

SCENARIOS = (
    "cpu-loop-sum",
    "cpu-fibs",
    "cpu-string-build",
    "cpu-hashmap",
    "tensor-create",
    "tensor-elementwise",
    "tensor-reduce",
    "tensor-matmul",
    "ml-mlp-step",
    "async-echo",
    "async-pipeline",
    "sort-int",
    "binary-search",
    "sieve",
    "matrix-transpose",
    "string-reverse",
    "count-primes",
    "gcd",
    "pow-fast",
    "word-count",
    "digit-sum",
)


def validate_scenario_ids(ids: list[str] | tuple[str, ...]) -> list[str]:
    """Return contract errors for a scenario collection."""
    errors: list[str] = []
    allowed = set(SCENARIOS)
    seen: set[str] = set()
    for scenario_id in ids:
        if scenario_id in seen:
            errors.append(f"duplicate scenario: {scenario_id}")
        seen.add(scenario_id)
        if scenario_id not in allowed:
            errors.append(f"unknown scenario: {scenario_id}")
    return errors
