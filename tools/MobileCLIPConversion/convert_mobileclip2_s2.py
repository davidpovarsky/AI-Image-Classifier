#!/usr/bin/env python3
"""Convert Apple's official MobileCLIP2-S2 checkpoint to two Core ML packages."""
from __future__ import annotations

import argparse
import datetime as dt
import json
import pathlib

import coremltools as ct
import numpy as np
import open_clip
import torch
from mobileclip.modules.common.mobileone import reparameterize_model

MODEL_ID = "apple/MobileCLIP2-S2"
MODEL_NAME = "MobileCLIP2-S2"
IMAGE_SIZE = 256
CONTEXT_LENGTH = 77
PROMPTS = {
    "woman": [
        "a photo of a woman", "a photo of a female person", "a photo of an adult woman",
        "a photo of a girl", "a photo of a female child", "a full body photo of a woman",
        "a cropped photo of a woman", "a person who appears female",
    ],
    "man": [
        "a photo of a man", "a photo of a male person", "a photo of an adult man",
        "a photo of a boy", "a photo of a male child", "a full body photo of a man",
        "a cropped photo of a man", "a person who appears male",
    ],
    "uncertain": [
        "a photo of a person whose gender is unclear", "a photo of an androgynous person",
        "a photo of a partially visible person", "a photo of a person with an obscured face",
        "a photo where the person's gender cannot be determined",
    ],
    "notPerson": [
        "a photo with no person", "an object", "an animal", "a landscape", "a building", "a vehicle",
    ],
}


class ImageEncoder(torch.nn.Module):
    def __init__(self, model: torch.nn.Module):
        super().__init__()
        self.model = model

    def forward(self, image: torch.Tensor) -> torch.Tensor:
        return self.model.encode_image(image, normalize=True)


class TextEncoder(torch.nn.Module):
    def __init__(self, model: torch.nn.Module):
        super().__init__()
        self.model = model

    def forward(self, text: torch.Tensor) -> torch.Tensor:
        return self.model.encode_text(text, normalize=True)


def normalized_mean(vectors: np.ndarray) -> np.ndarray:
    vectors = vectors / np.linalg.norm(vectors, axis=1, keepdims=True)
    mean = vectors.mean(axis=0)
    return mean / np.linalg.norm(mean)


def metadata(checkpoint: pathlib.Path) -> dict[str, str]:
    return {
        "com.github.apple.ml-mobileclip.source": "https://github.com/apple/ml-mobileclip",
        "com.github.apple.ml-mobileclip.checkpoint": f"https://huggingface.co/{MODEL_ID}",
        "com.github.apple.ml-mobileclip.license": "Apple ML Research Model License Agreement",
        "com.github.apple.ml-mobileclip.conversion_date": dt.datetime.now(dt.timezone.utc).isoformat(),
        "com.github.apple.ml-mobileclip.coremltools": ct.__version__,
        "com.github.apple.ml-mobileclip.torch": torch.__version__,
        "com.github.apple.ml-mobileclip.input": f"image RGB {IMAGE_SIZE}x{IMAGE_SIZE}; text int32 [1,{CONTEXT_LENGTH}]",
        "com.github.apple.ml-mobileclip.normalization": "image_mean=(0,0,0); image_std=(1,1,1); L2 embeddings",
        "com.github.apple.ml-mobileclip.tokenizer": "OpenCLIP CLIP tokenizer, context length 77",
        "com.github.apple.ml-mobileclip.checkpoint_file": checkpoint.name,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("AI-Image-Classifier/Models"))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)

    model, _, _ = open_clip.create_model_and_transforms(
        MODEL_NAME,
        pretrained=str(args.checkpoint),
        image_mean=(0, 0, 0),
        image_std=(1, 1, 1),
    )
    model.eval()
    model = reparameterize_model(model).eval()
    tokenizer = open_clip.get_tokenizer(MODEL_NAME)

    image_wrapper = ImageEncoder(model).eval()
    text_wrapper = TextEncoder(model).eval()
    image_example = torch.rand(1, 3, IMAGE_SIZE, IMAGE_SIZE)
    text_example = tokenizer(["a photo of a person"]).to(torch.int32)
    with torch.inference_mode():
        traced_image = torch.jit.trace(image_wrapper, image_example)
        traced_text = torch.jit.trace(text_wrapper, text_example)

    common = dict(
        convert_to="mlprogram",
        minimum_deployment_target=ct.target.iOS26,
        compute_precision=ct.precision.FLOAT32,
    )
    image_model = ct.convert(
        traced_image,
        inputs=[ct.ImageType(
            name="image", shape=image_example.shape, scale=1 / 255.0,
            bias=[0.0, 0.0, 0.0], color_layout=ct.colorlayout.RGB,
        )],
        outputs=[ct.TensorType(name="embedding")],
        **common,
    )
    text_model = ct.convert(
        traced_text,
        inputs=[ct.TensorType(name="text", shape=text_example.shape, dtype=np.int32)],
        outputs=[ct.TensorType(name="embedding")],
        **common,
    )
    for converted in (image_model, text_model):
        converted.author = "Converted from Apple MobileCLIP2-S2 by this repository"
        converted.license = "Apple Machine Learning Research Model License Agreement"
        converted.short_description = "MobileCLIP2-S2 normalized embedding encoder"
        converted.user_defined_metadata.update(metadata(args.checkpoint))

    image_path = args.output / "MobileCLIP2S2ImageEncoder.mlpackage"
    text_path = args.output / "MobileCLIP2S2TextEncoder.mlpackage"
    image_model.save(str(image_path))
    text_model.save(str(text_path))

    category_embeddings: dict[str, list[float]] = {}
    with torch.inference_mode():
        for category, prompts in PROMPTS.items():
            tokens = tokenizer(prompts).to(torch.int32)
            values = text_wrapper(tokens).cpu().numpy()
            category_embeddings[category] = normalized_mean(values).astype(float).tolist()
    prompt_hash = "\n".join(prompt for category in PROMPTS.values() for prompt in category)
    prompt_file = {
        "model": MODEL_NAME,
        "promptConfigurationHash": prompt_hash,
        "embeddings": category_embeddings,
    }
    (args.output / "MobileCLIP2S2PromptEmbeddings.json").write_text(
        json.dumps(prompt_file, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
