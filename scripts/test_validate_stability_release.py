"""Unit checks for the required/local release decision boundary."""

import importlib.util
from pathlib import Path


def load_module():
    path = Path(__file__).with_name("validate_stability_release.py")
    spec = importlib.util.spec_from_file_location("validate_stability_release", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_local_environment_skip_is_visible_but_not_a_failure():
    release = load_module()
    decision = release.release_decision(
        [{"id": "postgresql-16", "status": "skipped_environment"}],
        required=False,
    )
    assert decision == {
        "status": "skipped_environment",
        "release_certifying": False,
        "failures": [],
        "skipped_environment": ["postgresql-16"],
    }


def test_required_environment_skip_fails_closed():
    release = load_module()
    decision = release.release_decision(
        [{"id": "postgresql-16", "status": "skipped_environment"}],
        required=True,
    )
    assert decision["status"] == "failed"
    assert not decision["release_certifying"]


def test_any_gate_failure_is_not_certifying():
    release = load_module()
    decision = release.release_decision(
        [
            {"id": "cargo-fmt", "status": "passed"},
            {"id": "cargo-clippy", "status": "failed"},
        ],
        required=False,
    )
    assert decision["status"] == "failed"
    assert decision["failures"] == ["cargo-clippy"]
