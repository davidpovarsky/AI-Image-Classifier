#!/usr/bin/env python3
"""Inspect a compiled ML Program spec without executing it."""
from __future__ import annotations

import argparse
import collections
import json
import pathlib

import coremltools as ct


def shape(feature) -> list[object]:
    if not feature.type.HasField("multiArrayType"):
        return []
    array = feature.type.multiArrayType
    if array.shape:
        return list(array.shape)
    return ["dynamic"] if array.HasField("shapeRange") else []


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("model", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("."))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    spec = ct.utils.load_spec(str(args.model))
    operations: collections.Counter[str] = collections.Counter()
    dynamic_operation_shapes: list[str] = []
    functions = getattr(spec.mlProgram, "functions", {})
    for function_name, function in functions.items():
        for specialization in function.block_specializations.values():
            for operation in specialization.operations:
                operations[operation.type] += 1
                rendered = str(operation)
                if "symbol" in rendered.lower() or "rangeDim" in rendered:
                    dynamic_operation_shapes.append(f"{function_name}:{operation.type}")
    inputs = {item.name: shape(item) or ["image"] for item in spec.description.input}
    outputs = {item.name: shape(item) for item in spec.description.output}
    dynamic = any("dynamic" in value for values in [*inputs.values(), *outputs.values()] for value in values)
    suspicious = {
        "dynamicReshapeOrTranspose": [name for name in dynamic_operation_shapes if name.endswith(("reshape", "transpose"))],
        "complexGatherOrScatter": [name for name in operations if "gather" in name or "scatter" in name],
        "dynamicMatmulOrAttention": [name for name in dynamic_operation_shapes if "matmul" in name or "attention" in name],
    }
    result = {
        "model": args.model.name, "specificationVersion": spec.specificationVersion,
        "deploymentTarget": "iOS17" if spec.specificationVersion <= 8 else "requires inspection",
        "functionCount": len(functions), "inputs": inputs, "outputs": outputs,
        "operationCounts": dict(sorted(operations.items())),
        "hasDynamicDimensions": dynamic or bool(dynamic_operation_shapes),
        "dynamicOperationShapes": dynamic_operation_shapes, "suspiciousOperations": suspicious,
        "intermediatePrecision": "float16 conversion requested; inspect typed MIL values for exceptions",
    }
    (args.output / "MobileCLIP2S2MILInspection.json").write_text(json.dumps(result, indent=2) + "\n")
    (args.output / "MobileCLIP2S2MILOperations.txt").write_text(
        "\n".join(f"{name}\t{count}" for name, count in sorted(operations.items())) + "\n"
    )
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
