#!/usr/bin/env python3
"""Compare predictors on deterministic, text-bearing validation patches."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

import cv2
import numpy as np
import torch
from skimage.metrics import structural_similarity

from model import load_predictor


def load_patches(manifest: Path, patch: int, per_page: int, seed: int) -> tuple[np.ndarray, np.ndarray]:
    root = manifest.resolve().parent
    records = [item for item in json.loads(manifest.read_text()) if item["split"] == "validation"]
    if not records:
        raise SystemExit("manifest contains no validation records")
    rng = np.random.default_rng(seed)
    sources: list[np.ndarray] = []
    targets: list[np.ndarray] = []
    for record in records:
        source = cv2.cvtColor(cv2.imread(str(root / record["input"])), cv2.COLOR_BGR2RGB)
        target = cv2.cvtColor(cv2.imread(str(root / record["target"])), cv2.COLOR_BGR2RGB)
        height, width = target.shape[:2]
        accepted = 0
        for _ in range(per_page * 100):
            if accepted == per_page:
                break
            x = int(rng.integers(0, width - patch + 1))
            y = int(rng.integers(0, height - patch + 1))
            target_patch = target[y : y + patch, x : x + patch]
            if np.mean(np.min(target_patch, axis=2) < 210) < 0.004:
                continue
            sources.append(source[y : y + patch, x : x + patch])
            targets.append(target_patch)
            accepted += 1
    if not sources:
        raise SystemExit("could not sample text-bearing validation patches")
    return np.stack(sources), np.stack(targets)


def infer(network: torch.nn.Module, images: np.ndarray, batch: int, device: str) -> np.ndarray:
    results = []
    network.eval()
    with torch.inference_mode():
        for start in range(0, len(images), batch):
            tensor = torch.from_numpy(
                images[start : start + batch].astype(np.float32).transpose(0, 3, 1, 2) / 255.0
            ).to(device)
            prediction = torch.clamp(network(tensor), 0.0, 1.0)
            results.append(prediction.cpu().numpy().transpose(0, 2, 3, 1) * 255.0)
    return np.concatenate(results)


def metrics(prediction: np.ndarray, target: np.ndarray) -> dict[str, float]:
    difference = prediction.astype(np.float64) - target.astype(np.float64)
    mse = float(np.mean(difference**2))
    opponent_prediction = np.stack(
        (prediction[..., 0] - prediction[..., 1], prediction[..., 2] - prediction[..., 1]), axis=-1
    )
    opponent_target = np.stack(
        (target[..., 0] - target[..., 1], target[..., 2] - target[..., 1]), axis=-1
    )
    return {
        "mae": float(np.mean(np.abs(difference))),
        "psnr": 20.0 * math.log10(255.0 / math.sqrt(max(mse, 1e-12))),
        "ssim": float(
            np.mean(
                [
                    structural_similarity(target[index], prediction[index], channel_axis=2, data_range=255.0)
                    for index in range(len(target))
                ]
            )
        ),
        "chroma_mae": float(np.mean(np.abs(opponent_prediction - opponent_target))),
    }


def parse_model(value: str) -> tuple[str, Path]:
    if "=" not in value:
        raise argparse.ArgumentTypeError("model must be NAME=WEIGHTS")
    name, path = value.split("=", 1)
    if not name or not path:
        raise argparse.ArgumentTypeError("model must be NAME=WEIGHTS")
    return name, Path(path)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--naf-repo", type=Path, required=True)
    parser.add_argument("--model", action="append", type=parse_model, required=True, metavar="NAME=WEIGHTS")
    parser.add_argument("--patch", type=int, default=128)
    parser.add_argument("--per-page", type=int, default=20)
    parser.add_argument("--batch", type=int, default=8)
    parser.add_argument("--device", default="cpu")
    parser.add_argument("--seed", type=int, default=20260902)
    args = parser.parse_args()
    if args.device == "cpu":
        torch.set_num_threads(max(1, min(12, torch.get_num_threads())))
    source, target = load_patches(args.manifest, args.patch, args.per_page, args.seed)
    results = {"patches": len(source), "input": metrics(source.astype(np.float32), target.astype(np.float32))}
    print(f"input: {results['input']}", flush=True)
    for name, weights in args.model:
        prediction = infer(load_predictor(args.naf_repo, weights, args.device), source, args.batch, args.device)
        results[name] = metrics(prediction, target.astype(np.float32))
        print(f"{name}: {results[name]}", flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n")
    print(f"saved={args.output}")


if __name__ == "__main__":
    main()
