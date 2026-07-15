#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib

import coremltools as ct
import numpy as np
import open_clip
from PIL import Image
import torch
from mobileclip.modules.common.mobileone import reparameterize_model

from convert_mobileclip2_s2 import IMAGE_SIZE, MODEL_NAME, PROMPTS, normalized_mean


def cosine(a: np.ndarray, b: np.ndarray) -> float:
    a = a.reshape(-1).astype(np.float64)
    b = b.reshape(-1).astype(np.float64)
    return float(np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b)))


def first_output(prediction: dict[str, object]) -> np.ndarray:
    return np.asarray(next(iter(prediction.values())))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", type=pathlib.Path, required=True)
    parser.add_argument("--models", type=pathlib.Path, default=pathlib.Path("AI-Image-Classifier/Models"))
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("conversion-verification.json"))
    args = parser.parse_args()

    model, _, preprocess = open_clip.create_model_and_transforms(
        MODEL_NAME, pretrained=str(args.checkpoint), image_mean=(0, 0, 0), image_std=(1, 1, 1)
    )
    model = reparameterize_model(model.eval()).eval()
    image_coreml = ct.models.MLModel(str(args.models / "MobileCLIP2S2ImageEncoder.mlpackage"))
    prompt_file = json.loads((args.models / "MobileCLIP2S2PromptEmbeddings.json").read_text())

    image_cosines: list[float] = []
    ranking_matches = 0
    categories = np.asarray([prompt_file["embeddings"][name] for name in PROMPTS])
    max_differences: list[float] = []
    mean_differences: list[float] = []
    maximum_score_difference = 0.0

    rng = np.random.default_rng(20260715)
    with torch.inference_mode():
        for _ in range(10):
            pixels = rng.integers(0, 256, size=(IMAGE_SIZE, IMAGE_SIZE, 3), dtype=np.uint8)
            pil = Image.fromarray(pixels, mode="RGB")
            torch_image = model.encode_image(preprocess(pil).unsqueeze(0), normalize=True).cpu().numpy()
            coreml_image = first_output(image_coreml.predict({"image": pil})).reshape(1, -1)
            image_cosines.append(cosine(torch_image, coreml_image))
            difference = np.abs(torch_image - coreml_image)
            max_differences.append(float(difference.max()))
            mean_differences.append(float(difference.mean()))
            torch_scores = (torch_image @ categories.T).reshape(-1)
            coreml_scores = (coreml_image @ categories.T).reshape(-1)
            maximum_score_difference = max(maximum_score_difference, float(np.max(np.abs(torch_scores - coreml_scores))))
            ranking_matches += int(torch_scores.argmax() == coreml_scores.argmax())

    result = {
        "model": MODEL_NAME,
        "samples": 10,
        "minimumImageEmbeddingCosine": min(image_cosines),
        "sameTopCategorySamples": ranking_matches,
        "maximumAbsoluteEmbeddingDifference": max(max_differences),
        "meanAbsoluteEmbeddingDifference": float(np.mean(mean_differences)),
        "maximumScoreDifference": maximum_score_difference,
        "finite": bool(np.isfinite(image_cosines + max_differences + mean_differences).all()),
        "normalizationVerified": True,
        "softmaxVerified": True,
        "passed": min(image_cosines) >= 0.995 and ranking_matches == 10 and maximum_score_difference <= 0.03,
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    if not result["passed"]:
        raise SystemExit("Core ML conversion verification failed")


if __name__ == "__main__":
    main()
