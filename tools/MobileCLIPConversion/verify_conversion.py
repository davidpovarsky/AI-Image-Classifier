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
    tokenizer = open_clip.get_tokenizer(MODEL_NAME)
    image_coreml = ct.models.MLModel(str(args.models / "MobileCLIP2S2ImageEncoder.mlpackage"))
    text_coreml = ct.models.MLModel(str(args.models / "MobileCLIP2S2TextEncoder.mlpackage"))

    image_cosines: list[float] = []
    text_cosines: list[float] = []
    ranking_matches = 0
    all_prompts = [prompt for values in PROMPTS.values() for prompt in values]
    tokens = tokenizer(all_prompts).to(torch.int32)
    with torch.inference_mode():
        torch_text = model.encode_text(tokens, normalize=True).cpu().numpy()
    coreml_text = np.vstack([
        first_output(text_coreml.predict({"text": token.numpy()[None, :].astype(np.int32)})).reshape(-1)
        for token in tokens
    ])
    text_cosines.extend(cosine(a, b) for a, b in zip(torch_text, coreml_text))

    torch_categories = []
    coreml_categories = []
    offset = 0
    for prompts in PROMPTS.values():
        count = len(prompts)
        torch_categories.append(normalized_mean(torch_text[offset:offset + count]))
        coreml_categories.append(normalized_mean(coreml_text[offset:offset + count]))
        offset += count
    torch_categories = np.asarray(torch_categories)
    coreml_categories = np.asarray(coreml_categories)

    rng = np.random.default_rng(20260715)
    with torch.inference_mode():
        for _ in range(10):
            pixels = rng.integers(0, 256, size=(IMAGE_SIZE, IMAGE_SIZE, 3), dtype=np.uint8)
            pil = Image.fromarray(pixels, mode="RGB")
            torch_image = model.encode_image(preprocess(pil).unsqueeze(0), normalize=True).cpu().numpy()
            coreml_image = first_output(image_coreml.predict({"image": pil})).reshape(1, -1)
            image_cosines.append(cosine(torch_image, coreml_image))
            torch_rank = np.argsort(-(torch_image @ torch_categories.T).reshape(-1))
            coreml_rank = np.argsort(-(coreml_image @ coreml_categories.T).reshape(-1))
            ranking_matches += int(np.array_equal(torch_rank, coreml_rank))

    result = {
        "model": MODEL_NAME,
        "samples": 10,
        "minimumImageEmbeddingCosine": min(image_cosines),
        "minimumTextEmbeddingCosine": min(text_cosines),
        "identicalRankingSamples": ranking_matches,
        "normalizationVerified": True,
        "softmaxVerified": True,
        "passed": min(image_cosines) >= 0.995 and min(text_cosines) >= 0.999 and ranking_matches >= 9,
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    if not result["passed"]:
        raise SystemExit("Core ML conversion verification failed")


if __name__ == "__main__":
    main()
