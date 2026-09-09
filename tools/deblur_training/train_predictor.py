#!/usr/bin/env python3
"""Fine-tune the deterministic NAF document predictor from a pair manifest."""

from __future__ import annotations

import argparse
import json
import random
import time
from functools import lru_cache
from pathlib import Path

import cv2
import numpy as np
import torch

from model import load_predictor


@lru_cache(maxsize=12)
def read_rgb(path: str) -> np.ndarray:
    image = cv2.imread(path, cv2.IMREAD_COLOR)
    if image is None:
        raise RuntimeError(f"cannot read {path}")
    return cv2.cvtColor(image, cv2.COLOR_BGR2RGB).astype(np.float32) / 255.0


def gradient_loss(prediction: torch.Tensor, target: torch.Tensor) -> torch.Tensor:
    return torch.nn.functional.l1_loss(
        prediction[:, :, :, 1:] - prediction[:, :, :, :-1],
        target[:, :, :, 1:] - target[:, :, :, :-1],
    ) + torch.nn.functional.l1_loss(
        prediction[:, :, 1:, :] - prediction[:, :, :-1, :],
        target[:, :, 1:, :] - target[:, :, :-1, :],
    )


def ink_loss(prediction: torch.Tensor, target: torch.Tensor) -> torch.Tensor:
    # Emphasise visible ink without hard thresholding or forcing colour pages to grayscale.
    darkness = torch.clamp((0.90 - target.amin(dim=1, keepdim=True)) / 0.90, 0.0, 1.0)
    return torch.mean(torch.abs(prediction - target) * darkness)


def sample_patch(record: dict, root: Path, patch: int, rng: random.Random):
    source = read_rgb(str(root / record["input"]))
    target = read_rgb(str(root / record["target"]))
    if source.shape != target.shape:
        raise RuntimeError(f"shape mismatch: {record}")
    height, width = source.shape[:2]
    if min(height, width) < patch:
        raise RuntimeError(f"image smaller than patch={patch}: {record['target']}")
    selected = None
    for attempt in range(24):
        x = rng.randint(0, width - patch)
        y = rng.randint(0, height - patch)
        source_patch = source[y : y + patch, x : x + patch]
        target_patch = target[y : y + patch, x : x + patch]
        selected = source_patch, target_patch
        dark = np.mean(np.min(target_patch, axis=2) < 0.82)
        # Preserve some blank-page examples while preferring useful text patches.
        if dark >= 0.003 or rng.random() < 0.10 or attempt == 23:
            break
    return selected


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--naf-repo", type=Path, required=True)
    parser.add_argument("--initial-weights", type=Path, required=True)
    parser.add_argument("--steps", type=int, default=1200)
    parser.add_argument("--batch", type=int, default=4)
    parser.add_argument("--patch", type=int, default=128)
    parser.add_argument("--learning-rate", type=float, default=7e-5)
    parser.add_argument("--edge-weight", type=float, default=0.20)
    parser.add_argument("--ink-weight", type=float, default=0.35)
    parser.add_argument("--device", default="cpu")
    parser.add_argument("--seed", type=int, default=20260902)
    args = parser.parse_args()

    root = args.manifest.resolve().parent
    records = [item for item in json.loads(args.manifest.read_text()) if item["split"] == "train"]
    if not records:
        raise SystemExit("manifest contains no training records")
    torch.manual_seed(args.seed)
    rng = random.Random(args.seed)
    if args.device == "cpu":
        torch.set_num_threads(max(1, min(12, torch.get_num_threads())))
    network = load_predictor(args.naf_repo, args.initial_weights, args.device)
    network.train()
    optimizer = torch.optim.AdamW(network.parameters(), lr=args.learning_rate, weight_decay=1e-5)
    started = time.perf_counter()

    for step in range(1, args.steps + 1):
        source_batch, target_batch = [], []
        for _ in range(args.batch):
            source_patch, target_patch = sample_patch(rng.choice(records), root, args.patch, rng)
            source_batch.append(source_patch)
            target_batch.append(target_patch)
        source = torch.from_numpy(np.stack(source_batch).transpose(0, 3, 1, 2)).to(args.device)
        target = torch.from_numpy(np.stack(target_batch).transpose(0, 3, 1, 2)).to(args.device)
        optimizer.zero_grad(set_to_none=True)
        prediction = network(source)
        pixel = torch.nn.functional.l1_loss(prediction, target)
        edge = gradient_loss(prediction, target)
        ink = ink_loss(prediction, target)
        loss = pixel + args.edge_weight * edge + args.ink_weight * ink
        loss.backward()
        torch.nn.utils.clip_grad_norm_(network.parameters(), 1.0)
        optimizer.step()
        if step == 1 or step % 50 == 0 or step == args.steps:
            print(
                f"step={step}/{args.steps} loss={loss.item():.6f} pixel={pixel.item():.6f} "
                f"edge={edge.item():.6f} ink={ink.item():.6f} "
                f"elapsed={time.perf_counter() - started:.1f}s",
                flush=True,
            )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    metadata = {
        "manifest": str(args.manifest.resolve()),
        "initial_weights": str(args.initial_weights.resolve()),
        "steps": args.steps,
        "patch": args.patch,
        "edge_weight": args.edge_weight,
        "ink_weight": args.ink_weight,
        "seed": args.seed,
    }
    torch.save({"model_state_dict": network.state_dict(), "training": metadata}, args.output)
    args.output.with_suffix(".json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2) + "\n")
    print(f"saved={args.output} total_seconds={time.perf_counter() - started:.3f}")


if __name__ == "__main__":
    main()
