#!/usr/bin/env python3
"""One-command clean images -> synthetic pairs -> trained predictor -> optional ONNX."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


def run(arguments: list[str]) -> None:
    print("+", " ".join(arguments), flush=True)
    subprocess.run(arguments, check=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("clean_dir", type=Path)
    parser.add_argument("work_dir", type=Path)
    parser.add_argument("--naf-repo", type=Path, required=True)
    parser.add_argument("--initial-weights", type=Path, required=True)
    parser.add_argument("--variants", type=int, default=4)
    parser.add_argument("--steps", type=int, default=1200)
    parser.add_argument("--sigma-max", type=float, default=2.10)
    parser.add_argument("--scale-min", type=float, default=0.72)
    parser.add_argument("--ink-weight", type=float, default=0.35)
    parser.add_argument("--device", default="cpu")
    parser.add_argument("--skip-onnx", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    dataset = args.work_dir / "dataset"
    model = args.work_dir / "naf-document-gaussian.pth"
    run(
        [sys.executable, str(root / "prepare_pairs.py"), str(args.clean_dir), str(dataset),
         "--variants", str(args.variants), "--sigma-max", str(args.sigma_max),
         "--scale-min", str(args.scale_min)]
    )
    run(
        [sys.executable, str(root / "train_predictor.py"), str(dataset / "manifest.json"), str(model),
         "--naf-repo", str(args.naf_repo), "--initial-weights", str(args.initial_weights),
         "--steps", str(args.steps), "--ink-weight", str(args.ink_weight), "--device", args.device]
    )
    if not args.skip_onnx:
        run(
            [sys.executable, str(root / "export_onnx.py"), str(model), str(args.work_dir / "naf-document-gaussian-256.onnx"),
             "--naf-repo", str(args.naf_repo)]
        )
    print(f"model={model}")


if __name__ == "__main__":
    main()
