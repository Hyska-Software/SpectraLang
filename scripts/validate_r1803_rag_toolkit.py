#!/usr/bin/env python3
"""Validate R-1803 tokenization, embeddings, vector index, and RAG gates."""

from __future__ import annotations

import json
import hashlib
import struct
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ARTIFACT_HEADER = struct.Struct("<8sIQQ")
ARTIFACT_MAGIC = b"SPARART1"


def run_step(name: str, args: list[str]) -> None:
    print(f"[R-1803] {name}: {' '.join(args)}")
    completed = subprocess.run(args, cwd=ROOT, text=True)
    if completed.returncode != 0:
        raise SystemExit(completed.returncode)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def validate_index(path: Path) -> None:
    require(path.exists(), f"vector index artifact missing: {path}")
    data = path.read_bytes()
    require(len(data) >= ARTIFACT_HEADER.size + 32, "truncated vector index artifact")
    magic, version, manifest_len, payload_len = ARTIFACT_HEADER.unpack_from(data)
    require(magic == ARTIFACT_MAGIC and version == 1, "unsupported vector index artifact header")
    body_len = ARTIFACT_HEADER.size + manifest_len + payload_len
    require(len(data) == body_len + 32, "invalid vector index artifact length")
    require(
        hashlib.sha256(data[:body_len]).digest() == data[body_len:],
        "vector index artifact checksum mismatch",
    )

    manifest_start = ARTIFACT_HEADER.size
    manifest_end = manifest_start + manifest_len
    payload = data[manifest_end:body_len]
    manifest = json.loads(data[manifest_start:manifest_end].decode("utf-8"))
    require(
        manifest.get("schema") == "spectralang.artifact.v1"
        and manifest.get("format_version") == 1
        and manifest.get("kind") == "multi_array",
        "bad vector index artifact schema",
    )
    metadata = manifest.get("metadata", {})
    require(metadata.get("artifact_role") == "vector_index", "bad vector index role")
    require(metadata.get("index_type") == "hnsw", "bad vector index type")
    require(metadata.get("index_version") == "v2", "bad vector index version")
    require(metadata.get("metric") == "cosine", "bad vector index metric")
    require(metadata.get("dtype") == "f64", "bad vector index dtype")
    require(metadata.get("dimension") == "16", "example vector dimension mismatch")
    require(metadata.get("entry_count") == "2", "example vector entry count mismatch")
    ids = set(json.loads(metadata.get("ids_json", "[]")))
    require({"doc-rag", "doc-ml"}.issubset(ids), "expected RAG entries missing")

    arrays = {entry["name"]: entry for entry in manifest.get("arrays", [])}
    require(set(arrays) == {"vectors", "levels", "links"}, "vector index array set mismatch")
    for entry in arrays.values():
        start = entry["offset"]
        end = start + entry["length"]
        require(0 <= start <= end <= len(payload), "vector index array bounds mismatch")
        require(
            hashlib.sha256(payload[start:end]).hexdigest() == entry["checksum"],
            f"vector index array checksum mismatch: {entry['name']}",
        )
    require(arrays["vectors"]["shape"] == [2, 16], "vector array shape mismatch")
    require(arrays["levels"]["shape"] == [2], "level array shape mismatch")
    require(arrays["links"]["shape"] == [2, 1, 32], "link array shape mismatch")


def main() -> int:
    (ROOT / "target/ai-examples/rag").mkdir(parents=True, exist_ok=True)
    run_step(
        "runtime RAG toolkit",
        [
            "cargo",
            "test",
            "-p",
            "spectra-runtime",
            "ml_phase18_rag_tokenizer_vector_index_and_prompt_eval",
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
            "tests/validation/97_ml_phase18_rag_toolkit.spectra",
        ],
    )
    run_step(
        "AI RAG example",
        [
            "cargo",
            "run",
            "-p",
            "spectra-cli",
            "--",
            "run",
            "examples/ai/rag_retrieval_pipeline.spectra",
        ],
    )
    validate_index(ROOT / "target/ai-examples/rag/vector-index.spar")
    report = ROOT / "target/ai-examples/rag/report.txt"
    require(report.exists(), "RAG report missing")
    require("retrieved=doc-rag" in report.read_text(encoding="utf-8"), "RAG retrieval evidence missing")
    print("[R-1803] validation passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
