#!/usr/bin/env python3
"""Export diagnostic prefixes of MobileCLIP2-S2's discovered visual stages."""
from __future__ import annotations

import argparse
import json
import pathlib

import coremltools as ct
import open_clip
import torch
from mobileclip.modules.common.mobileone import reparameterize_model

from convert_mobileclip2_s2 import IMAGE_SIZE, MODEL_NAME


class Prefix(torch.nn.Module):
    def __init__(self, modules: list[torch.nn.Module]):
        super().__init__()
        self.modules_ = torch.nn.ModuleList(modules)

    def forward(self, value: torch.Tensor) -> torch.Tensor:
        for module in self.modules_:
            value = module(value)
        return value


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    model, _, _ = open_clip.create_model_and_transforms(
        MODEL_NAME, pretrained=str(args.checkpoint), image_mean=(0, 0, 0), image_std=(1, 1, 1)
    )
    visual = reparameterize_model(model.eval()).visual.eval()
    stages = list(visual.named_children())
    if len(stages) < 4:
        raise SystemExit(f"Visual encoder exposes only {len(stages)} top-level stages; refusing to invent stage boundaries")
    example = torch.rand(1, 3, IMAGE_SIZE, IMAGE_SIZE)
    report = []
    for index, requested_name in enumerate(["stem", "through-stage1", "through-stage2", "through-stage3"]):
        selected = [module for _, module in stages[: index + 1]]
        entry = {"name": requested_name, "sourceModules": [name for name, _ in stages[: index + 1]], "loadsOnDevice": None, "computeUnitResults": {}}
        try:
            traced = torch.jit.trace(Prefix(selected).eval(), example)
            converted = ct.convert(
                traced, inputs=[ct.TensorType(name="image", shape=example.shape)], convert_to="mlprogram",
                minimum_deployment_target=ct.target.iOS17, compute_precision=ct.precision.FLOAT16,
            )
            converted.save(str(args.output / f"MobileCLIP2S2-{requested_name}.mlpackage"))
            entry["conversionSucceeded"] = True
        except Exception as error:
            entry.update(conversionSucceeded=False, error=str(error))
        report.append(entry)
    report.append({"name": "full-encoder", "loadsOnDevice": None, "computeUnitResults": {}, "note": "Use convert_mobileclip2_s2.py for the full encoder"})
    (args.output / "partial-models.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
