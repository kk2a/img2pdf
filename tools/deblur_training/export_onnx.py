#!/usr/bin/env python3
"""Export a trained deterministic NAF predictor to fixed-tile ONNX."""

from __future__ import annotations

import argparse
from pathlib import Path

import torch

from model import load_predictor


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("weights", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--naf-repo", type=Path, required=True)
    parser.add_argument("--tile", type=int, default=256)
    args = parser.parse_args()
    network = load_predictor(args.naf_repo, args.weights).eval()
    example = torch.zeros(1, 3, args.tile, args.tile, dtype=torch.float32)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(
        network,
        (example,),
        args.output,
        input_names=["input"],
        output_names=["output"],
        dynamic_axes={"input": {0: "batch"}, "output": {0: "batch"}},
        opset_version=18,
        dynamo=False,
    )
    print(f"saved={args.output}")


if __name__ == "__main__":
    main()
