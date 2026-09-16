#!/usr/bin/env python3
"""Validate R-1801 ONNX import/export gates."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run_step(name: str, args: list[str]) -> None:
    print(f"[R-1801] {name}: {' '.join(args)}")
    completed = subprocess.run(args, cwd=ROOT, text=True)
    if completed.returncode != 0:
        raise SystemExit(completed.returncode)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def read_varint(data: bytes, index: int) -> tuple[int, int]:
    shift = 0
    value = 0
    while index < len(data):
        byte = data[index]
        index += 1
        value |= (byte & 0x7F) << shift
        if byte & 0x80 == 0:
            return value, index
        shift += 7
    raise ValueError("truncated varint")


def read_len(data: bytes, index: int) -> tuple[bytes, int]:
    length, index = read_varint(data, index)
    end = index + length
    if end > len(data):
        raise ValueError("truncated length-delimited field")
    return data[index:end], end


def skip(data: bytes, index: int, wire: int) -> int:
    if wire == 0:
        _, index = read_varint(data, index)
        return index
    if wire == 1:
        return index + 8
    if wire == 2:
        _, index = read_len(data, index)
        return index
    if wire == 5:
        return index + 4
    raise ValueError(f"unsupported wire type {wire}")


def node_ops(node: bytes) -> list[str]:
    ops: list[str] = []
    index = 0
    while index < len(node):
        key, index = read_varint(node, index)
        field = key >> 3
        wire = key & 0x7
        if field == 4 and wire == 2:
            raw, index = read_len(node, index)
            ops.append(raw.decode("utf-8"))
        else:
            index = skip(node, index, wire)
    return ops


def parse_onnx(path: Path) -> dict[str, object]:
    data = path.read_bytes()
    index = 0
    ops: list[str] = []
    graphs = 0
    inputs = 0
    outputs = 0
    opset = False
    while index < len(data):
        key, index = read_varint(data, index)
        field = key >> 3
        wire = key & 0x7
        if field == 7 and wire == 2:
            graph, index = read_len(data, index)
            graphs += 1
            gi = 0
            while gi < len(graph):
                gkey, gi = read_varint(graph, gi)
                gfield = gkey >> 3
                gwire = gkey & 0x7
                if gwire == 2:
                    payload, gi = read_len(graph, gi)
                    if gfield == 1:
                        ops.extend(node_ops(payload))
                    elif gfield == 11:
                        inputs += 1
                    elif gfield == 12:
                        outputs += 1
                else:
                    gi = skip(graph, gi, gwire)
        elif field == 8 and wire == 2:
            _, index = read_len(data, index)
            opset = True
        else:
            index = skip(data, index, wire)
    return {"graphs": graphs, "ops": ops, "inputs": inputs, "outputs": outputs, "opset": opset}


def validate_exported_models() -> None:
    expected = {
        "linear.onnx": {"Gemm"},
        "activation.onnx": {"Relu"},
        "normalization.onnx": {"LayerNormalization"},
        "transformer.onnx": {"MatMul", "Softmax", "LayerNormalization", "Gelu"},
        "transformer.roundtrip.onnx": {"MatMul", "Softmax", "LayerNormalization", "Gelu"},
    }
    for filename, required_ops in expected.items():
        path = ROOT / "target/r1801" / filename
        require(path.exists(), f"missing ONNX artifact {path}")
        parsed = parse_onnx(path)
        require(parsed["graphs"] == 1, f"{filename}: expected one graph")
        require(parsed["opset"] is True, f"{filename}: opset import missing")
        require(parsed["inputs"] > 0, f"{filename}: inputs missing")
        require(parsed["outputs"] > 0, f"{filename}: outputs missing")
        ops = set(parsed["ops"])
        require(required_ops.issubset(ops), f"{filename}: missing ops {required_ops - ops}")


def main() -> int:
    (ROOT / "target/ai-examples/onnx").mkdir(parents=True, exist_ok=True)
    run_step(
        "runtime ONNX subset API",
        [
            "cargo",
            "test",
            "-p",
            "spectra-runtime",
            "ml_phase18_onnx_subset_export_import_and_roundtrip",
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
            "tests/validation/95_ml_phase18_onnx_import_export.spectra",
        ],
    )
    run_step(
        "AI ONNX transformer example",
        [
            "cargo",
            "run",
            "-p",
            "spectra-cli",
            "--",
            "run",
            "examples/ai/onnx_transformer_export.spectra",
        ],
    )
    validate_exported_models()
    print("[R-1801] validation passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
