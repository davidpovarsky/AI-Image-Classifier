#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
import onnxruntime as ort
import open_clip
import torch
from mobileclip.modules.common.mobileone import reparameterize_model

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mobileclip_contract import load_ios_contract  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--onnx", type=Path, required=True)
    parser.add_argument("--prompts", type=Path, required=True)
    parser.add_argument("--ios-converter", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    contract = load_ios_contract(args.ios_converter)
    model, _, _ = open_clip.create_model_and_transforms(
        contract.model_name,
        pretrained=str(args.checkpoint),
        image_mean=(0, 0, 0),
        image_std=(1, 1, 1),
    )
    model = reparameterize_model(model.eval()).eval()
    session = ort.InferenceSession(str(args.onnx), providers=["CPUExecutionProvider"])
    bundle = np.load(args.prompts, allow_pickle=False)
    class_names = [str(value) for value in bundle["class_names"].tolist()]
    if class_names != contract.class_names:
        raise SystemExit("Prompt class order mismatch")
    prompts = np.asarray(bundle["embeddings"], dtype=np.float32)
    prompts /= np.linalg.norm(prompts, axis=1, keepdims=True)
    results = []
    passed = True
    for seed in range(10):
        generator = torch.Generator().manual_seed(seed)
        tensor = torch.rand(1, 3, contract.image_size, contract.image_size, generator=generator)
        with torch.inference_mode():
            source_output = model.encode_image(tensor, normalize=True).cpu().numpy()
        converted_output = np.asarray(session.run(["embedding"], {"image": tensor.numpy()})[0])
        expected_shape = (1, 512)
        shape_equal = source_output.shape == converted_output.shape == expected_shape
        source = source_output.reshape(-1)
        converted = converted_output.reshape(-1)
        converted /= max(float(np.linalg.norm(converted)), 1e-12)
        cosine = float(
            np.dot(source, converted) / (np.linalg.norm(source) * np.linalg.norm(converted))
        )
        source_logits = 100.0 * prompts @ source
        converted_logits = 100.0 * prompts @ converted
        ranking_equal = np.array_equal(np.argsort(source_logits), np.argsort(converted_logits))
        logits_equal = np.allclose(source_logits, converted_logits, rtol=1e-3, atol=1e-3)
        source_class = int(np.argmax(source_logits))
        converted_class = int(np.argmax(converted_logits))
        class_equal = source_class == converted_class
        source_norm = float(np.linalg.norm(source))
        converted_norm = float(np.linalg.norm(converted))
        finite = bool(
            np.isfinite(source).all()
            and np.isfinite(converted).all()
            and np.isfinite(source_logits).all()
            and np.isfinite(converted_logits).all()
        )
        sample_passed = (
            shape_equal
            and finite
            and abs(source_norm - 1.0) <= 1e-4
            and abs(converted_norm - 1.0) <= 1e-4
            and cosine >= 0.999
            and logits_equal
            and ranking_equal
            and class_equal
        )
        passed = passed and bool(sample_passed)
        results.append(
            {
                "seed": seed,
                "cosineSimilarity": cosine,
                "embeddingShape": list(converted_output.shape),
                "sourceEmbeddingNorm": source_norm,
                "onnxEmbeddingNorm": converted_norm,
                "sourceLogits": source_logits.tolist(),
                "onnxLogits": converted_logits.tolist(),
                "maxLogitAbsoluteError": float(np.max(np.abs(source_logits - converted_logits))),
                "logitsEqual": bool(logits_equal),
                "rankingEqual": bool(ranking_equal),
                "sourceClass": class_names[source_class],
                "onnxClass": class_names[converted_class],
                "classEqual": bool(class_equal),
                "finite": finite,
                "passed": bool(sample_passed),
            }
        )
    report = {"passed": passed, "sampleCount": len(results), "samples": results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if not passed:
        raise SystemExit("MobileCLIP ONNX parity failed")


if __name__ == "__main__":
    main()
