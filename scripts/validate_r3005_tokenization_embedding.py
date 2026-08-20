"""Independent R-3005 validator for tokenizer and embedding artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import subprocess
from pathlib import Path

MAGIC = b"SPARART1"
VERSION = 1
HEADER = struct.Struct("<8sIQQ")
DIGEST_SIZE = 32


def parse_container(raw: bytes) -> dict:
    if len(raw) < HEADER.size + DIGEST_SIZE:
        raise ValueError("truncated")
    magic, version, manifest_len, payload_len = HEADER.unpack_from(raw)
    if magic != MAGIC or version != VERSION:
        raise ValueError("header version")
    expected = HEADER.size + manifest_len + payload_len + DIGEST_SIZE
    if expected != len(raw):
        raise ValueError("length")
    if hashlib.sha256(raw[:-DIGEST_SIZE]).digest() != raw[-DIGEST_SIZE:]:
        raise ValueError("global checksum")
    start = HEADER.size
    manifest = json.loads(raw[start : start + manifest_len])
    allowed = {"schema", "format_version", "kind", "name", "model_version", "compatibility", "metadata", "arrays"}
    if set(manifest) != allowed or manifest["schema"] != "spectralang.artifact.v1" or manifest["format_version"] != VERSION:
        raise ValueError("manifest schema")
    if manifest["compatibility"] != {"container": "spectralang.artifact.v1", "tensor_encoding": "little-endian-f64-slots"}:
        raise ValueError("compatibility")
    if not isinstance(manifest["metadata"], dict) or any(not isinstance(k, str) or not isinstance(v, str) for k, v in manifest["metadata"].items()):
        raise ValueError("metadata")
    payload = raw[start + manifest_len : start + manifest_len + payload_len]
    names = set()
    ranges = []
    for array in manifest["arrays"]:
        required = {"name", "dtype", "precision", "shape", "layout", "offset", "length", "checksum"}
        if set(array) != required or not array["name"] or array["name"] in names:
            raise ValueError("array schema")
        names.add(array["name"])
        if array["dtype"] not in {"int", "float"} or array["precision"] != "f64" or array["layout"] != "contiguous":
            raise ValueError("array representation")
        if not array["shape"] or any(not isinstance(dim, int) or dim <= 0 for dim in array["shape"]):
            raise ValueError("shape")
        offset, length = array["offset"], array["length"]
        if offset < 0 or length < 0 or offset + length > len(payload):
            raise ValueError("range")
        element_count = 1
        for dimension in array["shape"]:
            element_count *= dimension
        if length != element_count * 8:
            raise ValueError("shape length")
        chunk = payload[offset : offset + length]
        if hashlib.sha256(chunk).hexdigest() != array["checksum"]:
            raise ValueError("array checksum")
        ranges.append((offset, offset + length))
    ranges.sort()
    if any(left[1] > right[0] for left, right in zip(ranges, ranges[1:])):
        raise ValueError("overlap")
    return manifest


def validate_tokenizer(manifest: dict) -> None:
    metadata = manifest["metadata"]
    if metadata.get("tokenizer_type") != "wordpiece" or metadata.get("tokenizer_version") != "v1":
        raise ValueError("tokenizer metadata")
    vocab = json.loads(metadata["vocab_json"])
    tokens = vocab["tokens"]
    ids = [item["id"] for item in tokens]
    names = [item["token"] for item in tokens]
    if ids != list(range(len(ids))) or len(set(names)) != len(names) or not all(names):
        raise ValueError("vocabulary ids/tokens")
    special = vocab["special_tokens"]
    if "unk" not in special or any(value not in ids for value in special.values()):
        raise ValueError("special tokens")
    if not isinstance(vocab["lowercase"], bool) or not vocab["continuation_prefix"]:
        raise ValueError("tokenizer options")
    token_ids = next(array for array in manifest["arrays"] if array["name"] == "token_ids")
    if token_ids["dtype"] != "int" or token_ids["shape"] != [len(tokens)]:
        raise ValueError("token id array")


def validate_embedding(manifest: dict) -> None:
    metadata = manifest["metadata"]
    if metadata.get("artifact_role") != "embedding_weights":
        raise ValueError("embedding role")
    if int(metadata.get("vocab_size", "0")) <= 0 or int(metadata.get("embedding_dim", "0")) <= 0:
        raise ValueError("embedding metadata")
    array = next(item for item in manifest["arrays"] if item["name"] == "embedding.weight")
    if array["dtype"] != "float" or array["shape"] != [int(metadata["vocab_size"]), int(metadata["embedding_dim"])]:
        raise ValueError("embedding tensor contract")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--fixtures-dir", type=Path, default=Path("tests/fixtures/r3005"))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    report = {
        "schema": "spectralang.r3005_tokenization_embedding.v1",
        "valid_artifacts": [],
        "rejected_artifacts": [],
        "tokenization_results": [],
        "embedding_results": [],
        "reference_comparisons": [],
        "fallback_checks": [],
        "failures": [],
    }
    valid_names = {"tokenizer-valid.spar", "embedding-valid.spar"}
    try:
        for path in sorted(args.fixtures_dir.glob("*.spar")):
            raw = path.read_bytes()
            try:
                manifest = parse_container(raw)
                if path.name.startswith("tokenizer-"):
                    validate_tokenizer(manifest)
                    report["tokenization_results"].append({"artifact": path.name, "passed": path.name in valid_names})
                else:
                    validate_embedding(manifest)
                    report["embedding_results"].append({"artifact": path.name, "passed": path.name in valid_names})
                if path.name in valid_names:
                    report["valid_artifacts"].append(path.name)
                else:
                    report["failures"].append(f"invalid artifact accepted: {path.name}")
            except (ValueError, KeyError, TypeError, json.JSONDecodeError):
                if path.name in valid_names:
                    report["failures"].append(f"valid artifact rejected: {path.name}")
                else:
                    report["rejected_artifacts"].append(path.name)

        result = subprocess.run([str(args.binary.resolve()), "run", str(args.fixture.resolve())], cwd=root, capture_output=True, text=True, timeout=30)
        report["reference_comparisons"].append({"fixture": str(args.fixture), "cli_exit_code": result.returncode, "expected": 0})
        if result.returncode != 0:
            report["failures"].append("production fixture failed")
        # The stdlib registration and implementation are split across
        # runtime/src/stdlib/*.rs.  Validate the production tree instead of
        # the former monolithic mod.rs file.
        source = "\n".join(
            path.read_text(encoding="utf-8")
            for path in sorted((root / "runtime" / "src" / "stdlib").glob("*.rs"))
        )
        report["fallback_checks"].append({"production_loaders_present": "std_ml_tokenizer_load" in source and "std_ml_embedding_load" in source, "hash_path_is_not_fixture": "text_embed" not in args.fixture.read_text(encoding="utf-8")})
        if "std_ml_tokenizer_load" not in source or "std_ml_embedding_load" not in source:
            report["failures"].append("production loader registration missing")
    except Exception as exc:
        report["failures"].append(str(exc))
    report["status"] = "passed" if not report["failures"] and set(report["valid_artifacts"]) == valid_names and len(report["rejected_artifacts"]) == 12 else "failed"
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "failures": report["failures"]}, sort_keys=True))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
