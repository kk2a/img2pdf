#!/usr/bin/env python3
"""Apply a trained NAF predictor to one image or a directory of images."""

from __future__ import annotations

import argparse
from pathlib import Path

import cv2
import numpy as np
import torch

from model import load_predictor, tiled_predict
from prepare_pairs import EXTENSIONS, image_files


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--naf-repo", type=Path, required=True)
    parser.add_argument("--weights", type=Path, required=True)
    parser.add_argument("--tile", type=int, default=256)
    parser.add_argument("--overlap", type=int, default=32)
    parser.add_argument("--strength", type=float, default=1.0, help="0=input, 1=full restoration")
    parser.add_argument("--device", default="cpu")
    args = parser.parse_args()
    if not 0 <= args.strength <= 1:
        raise SystemExit("strength must be in [0, 1]")
    sources = image_files(args.input)
    if not sources:
        raise SystemExit(f"no images found: {args.input}")
    if args.device == "cpu":
        torch.set_num_threads(max(1, min(12, torch.get_num_threads())))
    network = load_predictor(args.naf_repo, args.weights, args.device)
    for file_index, path in enumerate(sources, 1):
        bgr = cv2.imread(str(path), cv2.IMREAD_COLOR)
        if bgr is None:
            print(f"skip unreadable image: {path}")
            continue
        rgb = cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB).astype(np.float32) / 255.0

        def progress(index, total):
            if index == total or index % 10 == 0:
                print(f"[{file_index}/{len(sources)}] {path.name}: tiles {index}/{total}", flush=True)

        restored = tiled_predict(network, rgb, args.tile, args.overlap, args.device, progress)
        result = rgb * (1.0 - args.strength) + restored * args.strength
        result = cv2.cvtColor(np.uint8(np.clip(np.rint(result * 255), 0, 255)), cv2.COLOR_RGB2BGR)
        if args.input.is_file() and args.output.suffix.lower() in EXTENSIONS:
            output_path = args.output
        else:
            relative = Path(path.name) if args.input.is_file() else path.relative_to(args.input)
            output_path = (args.output / relative).with_suffix(".png")
        output_path.parent.mkdir(parents=True, exist_ok=True)
        cv2.imwrite(str(output_path), result)
        print(f"saved={output_path}")


if __name__ == "__main__":
    main()
