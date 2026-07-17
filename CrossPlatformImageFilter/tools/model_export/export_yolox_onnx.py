#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
from pathlib import Path

import torch


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--yolox-root", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--opset", type=int, default=18)
    parser.add_argument("--input-size", type=int, default=416)
    args = parser.parse_args()
    sys.path.insert(0, str(args.yolox_root.resolve()))
    from yolox.exp import get_exp

    exp = get_exp(None, "yolox-nano")
    exp.test_size = (args.input_size, args.input_size)
    model = exp.get_model()
    checkpoint = torch.load(args.checkpoint, map_location="cpu", weights_only=False)
    model.load_state_dict(checkpoint["model"])
    model.eval()
    model.head.decode_in_inference = True
    example = torch.zeros(1, 3, args.input_size, args.input_size)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(
        model,
        example,
        args.output,
        input_names=["images"],
        output_names=["output"],
        opset_version=args.opset,
        do_constant_folding=True,
    )


if __name__ == "__main__":
    main()
