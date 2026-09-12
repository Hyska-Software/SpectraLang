#!/usr/bin/env python3
"""Validate R-1703 distributed training foundation gates."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run_step(name: str, args: list[str]) -> None:
    print(f"[R-1703] {name}: {' '.join(args)}")
    completed = subprocess.run(args, cwd=ROOT, text=True)
    if completed.returncode != 0:
        raise SystemExit(completed.returncode)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def validate_checkpoint(path: Path) -> None:
    checkpoint = json.loads(path.read_text(encoding="utf-8"))
    require(
        checkpoint["schema"] == "spectra.ml.distributed_checkpoint.v2",
        "bad distributed checkpoint schema",
    )
    require(
        checkpoint["topology"] == "multi-thread",
        "unsupported topology recorded",
    )
    require(checkpoint["seed"] == 2026, "seed was not preserved")
    require(checkpoint["worker_count"] == 2, "worker count was not preserved")
    require(checkpoint["global_step"] == 1, "checkpoint global step should capture pre-resume state")
    require(checkpoint["interrupted_worker"] == 1, "interrupted worker was not recorded")
    require(len(checkpoint["workers"]) == 2, "checkpoint worker list length mismatch")
    require(checkpoint["workers"][1]["active"] is False, "interrupted worker should be inactive in checkpoint")


def main() -> int:
    (ROOT / "target/ai-examples").mkdir(parents=True, exist_ok=True)
    run_step(
        "runtime checkpoint and resume API",
        [
            "cargo",
            "test",
            "-p",
            "spectra-runtime",
            "ml_phase17_distributed_training_checkpoint_resume",
        ],
    )
    run_step(
        "public Spectra validation",
        [
            "cargo",
            "run",
            "-p",
            "spectra-cli",
            "--",
            "run",
            "tests/validation/94_ml_phase17_distributed_training.spectra",
        ],
    )
    run_step(
        "AI distributed example",
        [
            "cargo",
            "run",
            "-p",
            "spectra-cli",
            "--",
            "run",
            "examples/ai/distributed_training_checkpoint.spectra",
        ],
    )
    validate_checkpoint(ROOT / "target/ai-examples/distributed-run/checkpoint.json")
    print("[R-1703] validation passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
