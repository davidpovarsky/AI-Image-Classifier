#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
from pathlib import Path

import numpy as np
import open_clip
import torch
from mobileclip.modules.common.mobileone import reparameterize_model

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mobileclip_contract import load_ios_contract  # noqa: E402


class ImageEncoder(torch.nn.Module):
    def __init__(self, model: torch.nn.Module) -> None:
        super().__init__()
        self.model = model

    def forward(self, image: torch.Tensor) -> torch.Tensor:
        return self.model.encode_image(image, normalize=True)


class TextEncoder(torch.nn.Module):
    def __init__(self, model: torch.nn.Module) -> None:
        super().__init__()
        self.model = model

    def forward(self, text: torch.Tensor) -> torch.Tensor:
        return self.model.encode_text(text, normalize=True)


def normalized_mean(vectors: np.ndarray) -> np.ndarray:
    vectors = vectors / np.maximum(np.linalg.norm(vectors, axis=1, keepdims=True), 1e-12)
    mean = vectors.mean(axis=0)
    return mean / max(float(np.linalg.norm(mean)), 1e-12)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prompts-output", type=Path, required=True)
    parser.add_argument("--ios-converter", type=Path, required=True)
    parser.add_argument("--opset", type=int, default=18)
    args = parser.parse_args()
    contract = load_ios_contract(args.ios_converter)

    model, _, _ = open_clip.create_model_and_transforms(
        contract.model_name,
        pretrained=str(args.checkpoint),
        image_mean=(0, 0, 0),
        image_std=(1, 1, 1),
    )
    model = reparameterize_model(model.eval()).eval()
    image_encoder = ImageEncoder(model).eval()
    text_encoder = TextEncoder(model).eval()
    example = torch.rand(1, 3, contract.image_size, contract.image_size)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Torch 2.8's dynamo path emits local functions and then fails its own
    # opset-18 version-conversion pass; the legacy path exports opset 18 directly.
    torch.onnx.export(
        image_encoder,
        (example,),
        args.output,
        input_names=["image"],
        output_names=["embedding"],
        opset_version=args.opset,
        do_constant_folding=True,
        dynamo=False,
    )

    tokenizer = open_clip.get_tokenizer(contract.model_name)
    embeddings = []
    with torch.inference_mode():
        for name in contract.class_names:
            tokens = tokenizer(contract.prompts[name]).to(torch.int32)
            values = text_encoder(tokens).cpu().numpy()
            embeddings.append(normalized_mean(values).astype(np.float32))
    args.prompts_output.parent.mkdir(parents=True, exist_ok=True)
    np.savez(
        args.prompts_output,
        class_names=np.asarray(contract.class_names),
        embeddings=np.stack(embeddings),
    )


if __name__ == "__main__":
    main()
