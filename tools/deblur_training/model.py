"""Shared NAF predictor construction and tiled inference."""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import torch


def load_predictor(naf_repo: Path, weights: Path, device: str = "cpu") -> torch.nn.Module:
    repo = naf_repo.resolve()
    if not (repo / "Deblurring" / "model" / "NAFDPM.py").is_file():
        raise FileNotFoundError(f"NAF-DPM repository not found: {repo}")
    sys.path.insert(0, str(repo))
    from Deblurring.model.NAFDPM import NAFDPM

    network = NAFDPM(
        input_channels=6,
        output_channels=3,
        n_channels=32,
        middle_blk_num=1,
        enc_blk_nums=[1, 1, 1, 1],
        dec_blk_nums=[1, 1, 1, 1],
        mode=0,
    ).init_predictor.float()
    checkpoint = torch.load(weights, map_location="cpu", weights_only=False)
    network.load_state_dict(checkpoint.get("model_state_dict", checkpoint))
    return network.to(device)


def positions(length: int, tile: int, stride: int) -> list[int]:
    if length <= tile:
        return [0]
    values = list(range(0, length - tile + 1, stride))
    if values[-1] != length - tile:
        values.append(length - tile)
    return values


def tiled_predict(
    network: torch.nn.Module,
    rgb: np.ndarray,
    tile: int = 256,
    overlap: int = 32,
    device: str = "cpu",
    progress=None,
) -> np.ndarray:
    """Restore an HWC float32 RGB image in [0, 1]."""
    height, width = rgb.shape[:2]
    tile = min(tile, height, width)
    if tile <= overlap:
        raise ValueError("tile must be larger than overlap")
    ys = positions(height, tile, tile - overlap)
    xs = positions(width, tile, tile - overlap)
    wy = np.maximum(np.hanning(tile), 0.08)
    wx = np.maximum(np.hanning(tile), 0.08)
    weight = np.outer(wy, wx).astype(np.float32)[None]
    accum = np.zeros((3, height, width), np.float32)
    divisor = np.zeros((1, height, width), np.float32)
    total = len(xs) * len(ys)
    network.eval()
    with torch.inference_mode():
        for index, (y, x) in enumerate(((y, x) for y in ys for x in xs), 1):
            patch = rgb[y : y + tile, x : x + tile]
            tensor = torch.from_numpy(patch.transpose(2, 0, 1)).unsqueeze(0).to(device)
            restored = torch.clamp(network(tensor), 0.0, 1.0)[0].cpu().numpy()
            accum[:, y : y + tile, x : x + tile] += restored * weight
            divisor[:, y : y + tile, x : x + tile] += weight
            if progress is not None:
                progress(index, total)
    return (accum / np.maximum(divisor, 1e-6)).transpose(1, 2, 0)
